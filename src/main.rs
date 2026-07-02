#![no_std]
#![no_main]

mod spislave;

use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::i2c_slave::{self, Config, I2cSlave};
use embassy_rp::peripherals::{I2C1, UART1};
use embassy_rp::uart::{self, InterruptHandler as UartInterruptHandler, Uart};
use static_cell::StaticCell;
use crate::spislave::SpiSlaveError;

use {defmt_rtt as _, panic_probe as _};
use embassy_rp::pio::Pio;
use embassy_rp::pio::InterruptHandler as PioInterruptHandler;
use embassy_rp::peripherals::PIO0;
use spislave::SpiSlave;

bind_interrupts!(struct Irqs {
    UART1_IRQ => UartInterruptHandler<UART1>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>; 
});

async fn echo_gpio(input: &mut Input<'_>, output: &mut Output<'_>) {
    input.wait_for_any_edge().await;
    
    match input.get_level() {
        Level::High => output.set_low(),
        Level::Low  => output.set_high(),
    }
    defmt::info!("rx: {:02X}, tx: {:02X}", input.get_level(), output.get_output_level());
}

#[embassy_executor::task]
async fn gpio_task(mut input: Input<'static>, mut output: Output<'static>) {
    loop {
        echo_gpio(&mut input, &mut output).await;
    }
}

type SpiPeriph = SpiSlave<'static, PIO0, 0>;

static SPI_CELL: StaticCell<SpiPeriph> = StaticCell::new();

#[embassy_executor::task]
async fn spi_task(spi: &'static mut SpiPeriph) {
    let mut state = EchoState { pending: None };
    loop {
        if let Err(e) = echo_spi(spi, &mut state).await {
            defmt::error!("SPI error: {:?}", e);
        }
    }
}

struct EchoState {
    pending: Option<u8>,
}

async fn echo_spi(spi: &mut SpiPeriph, state: &mut EchoState) -> Result<(), SpiSlaveError> {
    let mut rx = [0u8; 1];

    let tx = [state.pending.unwrap_or(0x00)];

    spi.transfer(&mut rx, &tx).await?;

    state.pending = Some(rx[0] ^ 0xF);

    defmt::info!("rx: {:02X}, next_tx: {:02X}", rx[0], state.pending.unwrap_or(0));

    Ok(())
}

type I2cPeriph = I2cSlave<'static, I2C1>;

static I2C_CELL: StaticCell<I2cPeriph> = StaticCell::new();


#[embassy_executor::task]
async fn i2c_task(i2c: &'static mut I2cPeriph) {
    loop {
        if let Err(e) = echo_i2c(i2c).await {
            defmt::error!("I2C error: {:?}", e);
        }
    }
}

async fn echo_i2c(i2c: &mut I2cPeriph) -> Result<(), i2c_slave::Error> {
    let mut buf = [0u8; 8];

    match i2c.listen(&mut buf).await {
        Ok(i2c_slave::Command::WriteRead(_)) => {
            let response = [buf[0] ^ 0xF];
            match i2c.respond_and_fill(&response, 0x00).await {
                Ok(read_status) => defmt::info!("read status: {}", read_status),
                Err(e) => defmt::error!("respond error: {:?}", e),
            }
        }
        Ok(other) => defmt::warn!("Unexpected command: {:?}", other),
        Err(e) => defmt::error!("listen error: {:?}", e),
    }
    Ok(())
}

type UartPeriph = Uart<'static, uart::Async>;

static UART_CELL: StaticCell<UartPeriph> = StaticCell::new();

async fn echo_uart(uart: &mut UartPeriph) -> Result<(), uart::Error> {
    let mut buf = [0u8; 1];
    uart.read(&mut buf).await?;
    buf[0] = buf[0] ^ 0xF;
    uart.write(&buf).await?;
    Ok(())
}

#[embassy_executor::task]
async fn uart_task(uart: &'static mut UartPeriph) {
    loop {
        if let Err(e) = echo_uart(uart).await {
            defmt::error!("UART error: {:?}", e);
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    
    let Pio {
        mut common,
        sm0,
        ..
    } = Pio::new(p.PIO0, Irqs);

    let input  = Input::new(p.PIN_6, Pull::Down);
    let output = Output::new(p.PIN_7, Level::Low);
    
    let spi = SpiSlave::new(
        &mut common, sm0,
        p.PIN_10, // miso
        p.PIN_11, // mosi
        p.PIN_12, // sck
        p.PIN_13, // cs
    );
    let spi_ref = SPI_CELL.init(spi);
    
    let mut i2c_config = Config::default();
    i2c_config.addr = 0x08;
    let i2c = I2cSlave::new(
        p.I2C1, p.PIN_3, p.PIN_2,
        Irqs, i2c_config
    );
    let i2c_ref = I2C_CELL.init(i2c);

    let uart = Uart::new(
            p.UART1, p.PIN_4, p.PIN_5,
            Irqs, p.DMA_CH2, p.DMA_CH3,
            uart::Config::default()
    );
    let uart_ref = UART_CELL.init(uart);
        

    spawner.spawn(gpio_task(input, output)).unwrap();
    spawner.spawn(spi_task(spi_ref)).unwrap();
    spawner.spawn(i2c_task(i2c_ref)).unwrap();
    spawner.spawn(uart_task(uart_ref)).unwrap();
}
