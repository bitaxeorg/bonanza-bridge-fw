#![no_std]
#![no_main]

use defmt::unwrap;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    gpio::{self},
    peripherals::{PIO1, UART0, UART1},
    pio::{self},
    pwm::{self},
    uart as rp_uart,
};
use embassy_time::Timer;
use static_cell::StaticCell;

mod control;
mod pio_uart;
mod uart;

const CONTROL_BAUDRATE: u32 = 115_200;
const DATA_BAUDRATE: u32 = 5_000_000;

bind_interrupts!(struct Irqs {
    UART0_IRQ => rp_uart::BufferedInterruptHandler<UART0>;
    UART1_IRQ => rp_uart::BufferedInterruptHandler<UART1>;
    PIO1_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut watchdog = embassy_rp::watchdog::Watchdog::new(p.WATCHDOG);
    watchdog.set_scratch(0, 0);
    watchdog.feed();

    let gpio_pins = control::gpio::Pins {
        v5_en: gpio::Output::new(p.PIN_18, gpio::Level::Low),
        asic_rst: gpio::Output::new(p.PIN_11, gpio::Level::High),
        asic_trip: gpio::Input::new(p.PIN_10, gpio::Pull::None),
    };

    let fan_pins = {
        let mut pwm_config = pwm::Config::default();
        pwm_config.top = 1000; // 1000 steps for 0.1% resolution
        pwm_config.compare_a = 0; // Start at 0% duty cycle
        pwm_config.compare_b = 0;
        pwm_config.divider = 5.into(); // 125MHz / 5 / 1000 = 25kHz
        pwm_config.invert_a = false;
        pwm_config.phase_correct = false;
        pwm_config.enable = true; // Explicitly enable PWM

        let pwm = pwm::Pwm::new_output_a(p.PWM_SLICE2, p.PIN_20, pwm_config.clone());

        let tach = gpio::Input::new(p.PIN_21, gpio::Pull::None);
        control::fan::Pins { pwm, tach }
    };

    let control_uart = {
        static TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
        static RX_BUF: StaticCell<[u8; 256]> = StaticCell::new();

        let mut config = rp_uart::Config::default();
        config.baudrate = CONTROL_BAUDRATE;

        rp_uart::BufferedUart::new(p.UART0, Irqs, p.PIN_0, p.PIN_1, TX_BUF.init([0; 256]), RX_BUF.init([0; 256]), config)
    };

    let data_uart = {
        static TX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
        static RX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();

        let mut config = rp_uart::Config::default();
        config.baudrate = DATA_BAUDRATE;

        rp_uart::BufferedUart::new(p.UART1, Irqs, p.PIN_4, p.PIN_5, TX_BUF.init([0; 4096]), RX_BUF.init([0; 4096]), config)
    };

    let pio::Pio { mut common, sm0, sm1, .. } = pio::Pio::new(p.PIO1, Irqs);
    let asic_uart = pio_uart::PioUart::new(&mut common, sm0, sm1, p.PIN_8, p.PIN_9, DATA_BAUDRATE);

    unwrap!(spawner.spawn(control::uart_task(control_uart, gpio_pins, fan_pins)));
    unwrap!(spawner.spawn(uart::uart_task(data_uart, asic_uart)));

    loop {
        watchdog.feed();
        Timer::after_secs(2).await;
    }
}
