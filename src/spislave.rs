#![allow(dead_code)]

use core::marker::PhantomData;
use embassy_rp::pio::{
    Common, Config, Instance, PioPin,
    ShiftConfig, ShiftDirection, StateMachine,
};
use embassy_rp::pio::program::pio_asm;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Error {
    TxFIFOFull,
    RxFIFOEmpty,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::TxFIFOFull => write!(f, "TX FIFO full"),
            Error::RxFIFOEmpty => write!(f, "RX FIFO empty"),
        }
    }
}

pub type SpiSlaveError = Error;
impl core::error::Error for Error {}

pub struct SpiSlave<'d, PIO: Instance, const SM: usize> {
    _pio: PhantomData<&'d PIO>,
    sm: StateMachine<'d, PIO, SM>,
    miso: embassy_rp::pio::Pin<'d, PIO>,
    mosi: embassy_rp::pio::Pin<'d, PIO>,
    sck:  embassy_rp::pio::Pin<'d, PIO>,
    cs:   embassy_rp::pio::Pin<'d, PIO>,
    loaded: embassy_rp::pio::LoadedProgram<'d, PIO>,
}

impl<'d, PIO: Instance, const SM: usize> SpiSlave<'d, PIO, SM> {
    pub fn new(
        common: &mut Common<'d, PIO>,
        mut sm: StateMachine<'d, PIO, SM>,
        miso: embassy_rp::Peri<'d, impl PioPin>,
        mosi: embassy_rp::Peri<'d, impl PioPin>,
        sck:  embassy_rp::Peri<'d, impl PioPin>,
        cs:   embassy_rp::Peri<'d, impl PioPin>,
    ) -> Self {
        let miso = common.make_pio_pin(miso);
        let mosi = common.make_pio_pin(mosi);
        let sck  = common.make_pio_pin(sck);
        let cs   = common.make_pio_pin(cs);

        let prg = pio_asm!(
                // Mode 0:
                // - change MISO while SCK is low
                // - sample MOSI on rising edge
                ".wrap_target",
                "start:",
                "    set pindirs, 0",   // tri-state MISO
                "    jmp pin start",    // wait for CS low
                "    set pindirs, 1",   // drive MISO
                "    pull",             // load first TX byte
                "    set x, 7",         // for(x=0;x<=7;x++){}
                "bitloop:",
                "    out pins, 1",      // shift 1bit OSR > MISO
                "    wait 1 pin 1",     // wait for sck rising edge
                "    in  pins, 1",      // sample 1bit MOSI > ISR
                "    wait 0 pin 1",     // wait for sck falling edge
                "    jmp pin start",    // if CS = HIGH end transaction
                "    jmp x-- bitloop",  // still here? continue transaction
                ".wrap",
            );

        // PIO instruction memory
        let loaded = common.load_program(&prg.program);

        let mut cfg = Config::default();

        cfg.use_program(&loaded, &[]);
        
        cfg.set_in_pins(&[&mosi, &sck]);
        cfg.set_out_pins(&[&miso]);
        cfg.set_set_pins(&[&miso]);
        cfg.set_jmp_pin(&cs);
            
        cfg.shift_in = ShiftConfig {
            auto_fill: true,                // ISR > RXFIFO automatically 
            threshold: 8,                   // once we get to 8 bits
            direction: ShiftDirection::Left,
        };

        cfg.shift_out = ShiftConfig {
            auto_fill: true,                // OSR > TXFIFO automatically 
            threshold: 8,                   // once we get to 8 bits
            direction: ShiftDirection::Left,
        };

        sm.set_config(&cfg);
        sm.set_pin_dirs(embassy_rp::pio::Direction::In, &[&mosi, &miso, &sck, &cs]);
        sm.set_enable(false);

        Self {
            _pio: PhantomData,
            sm,
            miso,
            mosi,
            sck,
            cs,
            loaded,
        }
    }

    pub async fn transfer(
        &mut self,
        read: &mut [u8],
        write: &[u8],
    ) -> Result<(), Error> {
        if read.is_empty() && write.is_empty() {
            return Ok(());
        }

        let data_len = read.len().max(write.len());

        self.sm.clear_fifos();

        // pre-fill
        // (prevents race conditions. The OSR is hungry and will not wait for us).
        let mut tx_idx = 0;
        while tx_idx < write.len() && tx_idx < 4 {
            self.sm.tx().wait_push((write[tx_idx] as u32) << 24).await;
            tx_idx += 1;
        }

        // we can't send nothing
        if write.is_empty() {
            self.sm.tx().wait_push(0).await;
        }

        self.sm.set_enable(true);

        for i in 0..data_len {
            // Receive
            let rx_word = self.sm.rx().wait_pull().await;
            if i < read.len() {
                // Store the single lowbyte
                read[i] = (rx_word & 0xFF) as u8;
            }

            if tx_idx < write.len() {
                // Send whatever data we have stored.
                self.sm.tx().wait_push((write[tx_idx] as u32) << 24).await;
                tx_idx += 1;
            }
        }

        self.sm.set_enable(false);

        Ok(())
    }
}
