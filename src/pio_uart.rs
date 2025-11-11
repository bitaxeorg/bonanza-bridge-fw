use embassy_rp::pio::{
    Config, Direction, FifoJoin, Instance, PioPin, ShiftDirection, StateMachine,
};

/// PIO-based 9N1 UART (9 data bits, no parity, 1 stop bit)
pub struct PioUart<'d, PIO: Instance, const SM_TX: usize, const SM_RX: usize> {
    sm_tx: StateMachine<'d, PIO, SM_TX>,
    sm_rx: StateMachine<'d, PIO, SM_RX>,
}

impl<'d, PIO: Instance, const SM_TX: usize, const SM_RX: usize> PioUart<'d, PIO, SM_TX, SM_RX> {
    pub fn new(
        pio: &mut embassy_rp::pio::Common<'d, PIO>,
        mut sm_tx: StateMachine<'d, PIO, SM_TX>,
        mut sm_rx: StateMachine<'d, PIO, SM_RX>,
        tx_pin: impl PioPin,
        rx_pin: impl PioPin,
        baudrate: u32,
    ) -> Self {
        // PIO program for 9-bit UART TX
        // Sends 1 start bit, 9 data bits, 1 stop bit = 11 bits total
        let prg_tx = pio_proc::pio_asm!(
            ".side_set 1 opt"
            ".wrap_target"
            "pull       side 1 [7]",  // Pull data from FIFO, drive line high (idle), delay
            "set x, 8   side 0 [7]",  // Set bit counter to 8 (9 bits: 0-8), send start bit
            "bitloop:",
            "out pins, 1       [6]",  // Shift out 1 bit
            "jmp x-- bitloop   [6]",  // Loop for 9 data bits
            "nop        side 1 [7]",  // Send stop bit (line high)
            ".wrap"
        );

        // PIO program for 9-bit UART RX
        // Receives 1 start bit, 9 data bits, 1 stop bit
        let prg_rx = pio_proc::pio_asm!(
            ".wrap_target"
            "wait 0 pin 0",          // Wait for start bit (low)
            "set x, 8     [10]",     // Set bit counter to 8 (9 bits), delay to middle of start bit
            "bitloop:",
            "in pins, 1   [6]",      // Sample and shift in 1 bit
            "jmp x-- bitloop [6]",   // Loop for 9 data bits
            "push",                  // Push received data to FIFO
            ".wrap"
        );

        // Install TX program
        let tx_pin = pio.make_pio_pin(tx_pin);
        sm_tx.set_pin_dirs(Direction::Out, &[&tx_pin]);
        let mut cfg_tx = Config::default();
        let prg_tx_loaded = pio.load_program(&prg_tx.program);
        cfg_tx.use_program(&prg_tx_loaded, &[&tx_pin]);
        cfg_tx.set_out_pins(&[&tx_pin]);
        cfg_tx.set_set_pins(&[&tx_pin]);
        cfg_tx.shift_out.direction = ShiftDirection::Right;
        cfg_tx.shift_out.auto_fill = false;
        cfg_tx.shift_out.threshold = 32;
        cfg_tx.fifo_join = FifoJoin::TxOnly;
        
        // Calculate clock divider for baudrate
        cfg_tx.clock_divider = Self::calculate_clk_div(baudrate);
        
        sm_tx.set_config(&cfg_tx);
        sm_tx.set_enable(true);

        // Install RX program
        let rx_pin = pio.make_pio_pin(rx_pin);
        sm_rx.set_pin_dirs(Direction::In, &[&rx_pin]);
        let mut cfg_rx = Config::default();
        let prg_rx_loaded = pio.load_program(&prg_rx.program);
        cfg_rx.use_program(&prg_rx_loaded, &[]);
        cfg_rx.set_in_pins(&[&rx_pin]);
        cfg_rx.shift_in.direction = ShiftDirection::Right;
        cfg_rx.shift_in.auto_fill = false;
        cfg_rx.shift_in.threshold = 32;
        cfg_rx.fifo_join = FifoJoin::RxOnly;
        cfg_rx.clock_divider = Self::calculate_clk_div(baudrate);
        
        sm_rx.set_config(&cfg_rx);
        sm_rx.set_enable(true);

        Self { sm_tx, sm_rx }
    }

    fn calculate_clk_div(baudrate: u32) -> fixed::FixedU32<fixed::types::extra::U8> {
        // RP2040 system clock is typically 125 MHz
        // Each bit takes 8 cycles in our PIO program
        // clk_div = sys_clk / (baudrate * cycles_per_bit)
        let sys_clk = 125_000_000u32;
        let cycles_per_bit = 8u32;
        // Compute division in fixed point: (sys_clk * 256) / (baudrate * cycles_per_bit)
        let numerator = (sys_clk as u64) * 256;
        let denominator = (baudrate as u64) * (cycles_per_bit as u64);
        let div_fixed = (numerator / denominator) as u32;
        fixed::FixedU32::from_bits(div_fixed)
    }

    pub fn set_baudrate(&mut self, baudrate: u32) {
        let clk_div = Self::calculate_clk_div(baudrate);
        self.sm_tx.set_clock_divider(clk_div);
        self.sm_rx.set_clock_divider(clk_div);
    }

    /// Write a 9-bit value (blocking)
    pub async fn write_u16(&mut self, data: u16) {
        // Mask to 9 bits
        let data = data & 0x1FF;
        self.sm_tx.tx().wait_push(data as u32).await;
    }

    /// Read a 9-bit value (blocking)
    pub async fn read_u16(&mut self) -> u16 {
        let data = self.sm_rx.rx().wait_pull().await;
        (data & 0x1FF) as u16
    }

    /// Check if TX FIFO is full
    pub fn tx_is_full(&mut self) -> bool {
        self.sm_tx.tx().full()
    }

    /// Check if RX FIFO is empty
    pub fn rx_is_empty(&mut self) -> bool {
        self.sm_rx.rx().empty()
    }

    /// Try to write a 9-bit value (non-blocking)
    pub fn try_write(&mut self, data: u16) -> bool {
        if !self.tx_is_full() {
            let data = data & 0x1FF;
            self.sm_tx.tx().push(data as u32);
            true
        } else {
            false
        }
    }

    /// Try to read a 9-bit value (non-blocking)
    pub fn try_read(&mut self) -> Option<u16> {
        if let Some(data) = self.sm_rx.rx().try_pull() {
            Some((data & 0x1FF) as u16)
        } else {
            None
        }
    }

    pub fn split(self) -> (PioUartTx<'d, PIO, SM_TX>, PioUartRx<'d, PIO, SM_RX>) {
        (
            PioUartTx { sm: self.sm_tx },
            PioUartRx { sm: self.sm_rx },
        )
    }
}

pub struct PioUartTx<'d, PIO: Instance, const SM: usize> {
    sm: StateMachine<'d, PIO, SM>,
}

impl<'d, PIO: Instance, const SM: usize> PioUartTx<'d, PIO, SM> {
    pub async fn write_u16(&mut self, data: u16) {
        let data = data & 0x1FF;
        self.sm.tx().wait_push(data as u32).await;
    }

    pub fn try_write(&mut self, data: u16) -> bool {
        if !self.is_full() {
            let data = data & 0x1FF;
            self.sm.tx().push(data as u32);
            true
        } else {
            false
        }
    }

    pub fn is_full(&mut self) -> bool {
        self.sm.tx().full()
    }
}

pub struct PioUartRx<'d, PIO: Instance, const SM: usize> {
    sm: StateMachine<'d, PIO, SM>,
}

impl<'d, PIO: Instance, const SM: usize> PioUartRx<'d, PIO, SM> {
    pub async fn read_u16(&mut self) -> u16 {
        let data = self.sm.rx().wait_pull().await;
        (data & 0x1FF) as u16
    }

    pub fn try_read(&mut self) -> Option<u16> {
        self.sm.rx().try_pull().map(|data| (data & 0x1FF) as u16)
    }

    pub fn is_empty(&mut self) -> bool {
        self.sm.rx().empty()
    }
}
