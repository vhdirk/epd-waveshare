//! A simple Driver for the Waveshare 1.54" (C) E-Ink Display via SPI

use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{delay::DelayUs, digital::Wait, spi::SpiDevice};

use crate::interface::DisplayInterface;
use crate::traits::{
    InternalWiAdditions, RefreshLut, WaveshareDisplay, WaveshareThreeColorDisplay,
};

/// Width of epd1in54 in pixels
pub const WIDTH: u32 = 152;
/// Height of epd1in54 in pixels
pub const HEIGHT: u32 = 152;
/// Default Background Color (white)
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::White;
const IS_BUSY_LOW: bool = true;
const NUM_DISPLAY_BITS: u32 = WIDTH * HEIGHT / 8;
const SINGLE_BYTE_WRITE: bool = true;

use crate::color::Color;

pub(crate) mod command;
use self::command::Command;
use crate::buffer_len;

/// Full size buffer for use with the 1in54c EPD
/// TODO this should be a TriColor, but let's keep it as is at first
#[cfg(feature = "graphics")]
pub type Display1in54c = crate::graphics::Display<
    WIDTH,
    HEIGHT,
    false,
    { buffer_len(WIDTH as usize, HEIGHT as usize) },
    Color,
>;

/// Epd1in54c driver
pub struct Epd1in54c<SPI, BUSY, DC, RST, DELAY> {
    interface: DisplayInterface<SPI, BUSY, DC, RST, DELAY, SINGLE_BYTE_WRITE>,
    color: Color,
}

impl<SPI, BUSY, DC, RST, DELAY> InternalWiAdditions<SPI, BUSY, DC, RST, DELAY>
    for Epd1in54c<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin + Wait,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs,
{
    async fn init(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        // Based on Reference Program Code from:
        // https://www.waveshare.com/w/upload/a/ac/1.54inch_e-Paper_Module_C_Specification.pdf
        // and:
        // https://github.com/waveshare/e-Paper/blob/master/STM32/STM32-F103ZET6/User/e-Paper/EPD_1in54c.c
        self.interface.reset(delay, 10_000, 2_000).await;

        // start the booster
        self.cmd_with_data(spi, Command::BoosterSoftStart, &[0x17, 0x17, 0x17])
            .await?;

        // power on
        self.command(spi, Command::PowerOn).await?;
        delay.delay_us(5000).await;
        self.wait_until_idle(spi, delay).await?;

        // set the panel settings
        self.cmd_with_data(spi, Command::PanelSetting, &[0x0f, 0x0d])
            .await?;

        // set resolution
        self.send_resolution(spi).await?;

        self.cmd_with_data(spi, Command::VcomAndDataIntervalSetting, &[0x77])
            .await?;

        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> WaveshareThreeColorDisplay<SPI, BUSY, DC, RST, DELAY>
    for Epd1in54c<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin + Wait,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs,
{
    async fn update_color_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        black: &[u8],
        chromatic: &[u8],
    ) -> Result<(), SPI::Error> {
        self.update_achromatic_frame(spi, delay, black).await?;
        self.update_chromatic_frame(spi, delay, chromatic).await
    }

    async fn update_achromatic_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        black: &[u8],
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay).await?;
        self.cmd_with_data(spi, Command::DataStartTransmission1, black)
            .await?;

        Ok(())
    }

    async fn update_chromatic_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        chromatic: &[u8],
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay).await?;
        self.cmd_with_data(spi, Command::DataStartTransmission2, chromatic)
            .await?;

        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> WaveshareDisplay<SPI, BUSY, DC, RST, DELAY>
    for Epd1in54c<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin + Wait,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs,
{
    type DisplayColor = Color;
    async fn new(
        spi: &mut SPI,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay: &mut DELAY,
        delay_us: Option<u32>,
    ) -> Result<Self, SPI::Error> {
        let interface = DisplayInterface::new(busy, dc, rst, delay_us);
        let color = DEFAULT_BACKGROUND_COLOR;

        let mut epd = Epd1in54c { interface, color };

        epd.init(spi, delay).await?;

        Ok(epd)
    }

    async fn sleep(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay).await?;

        self.command(spi, Command::PowerOff).await?;
        self.wait_until_idle(spi, delay).await?;
        self.cmd_with_data(spi, Command::DeepSleep, &[0xa5]).await?;

        Ok(())
    }

    async fn wake_up(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.init(spi, delay).await
    }

    fn set_background_color(&mut self, color: Color) {
        self.color = color;
    }

    fn background_color(&self) -> &Color {
        &self.color
    }

    fn width(&self) -> u32 {
        WIDTH
    }

    fn height(&self) -> u32 {
        HEIGHT
    }

    async fn update_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_achromatic_frame(spi, delay, buffer).await?;

        // Clear the chromatic layer
        let color = self.color.get_byte_value();

        self.command(spi, Command::DataStartTransmission2).await?;
        self.interface
            .data_x_times(spi, color, NUM_DISPLAY_BITS)
            .await?;

        Ok(())
    }

    #[allow(unused)]
    async fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        unimplemented!()
    }

    async fn display_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.command(spi, Command::DisplayRefresh).await?;
        self.wait_until_idle(spi, delay).await?;

        Ok(())
    }

    async fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_frame(spi, buffer, delay).await?;
        self.display_frame(spi, delay).await?;

        Ok(())
    }

    async fn clear_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay).await?;
        let color = DEFAULT_BACKGROUND_COLOR.get_byte_value();

        // Clear the black
        self.command(spi, Command::DataStartTransmission1).await?;
        self.interface
            .data_x_times(spi, color, NUM_DISPLAY_BITS)
            .await?;

        // Clear the chromatic
        self.command(spi, Command::DataStartTransmission2).await?;
        self.interface
            .data_x_times(spi, color, NUM_DISPLAY_BITS)
            .await?;

        Ok(())
    }

    async fn set_lut(
        &mut self,
        _spi: &mut SPI,
        _delay: &mut DELAY,
        _refresh_rate: Option<RefreshLut>,
    ) -> Result<(), SPI::Error> {
        Ok(())
    }

    async fn wait_until_idle(
        &mut self,
        _spi: &mut SPI,
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.interface.wait_until_idle(delay, IS_BUSY_LOW).await;
        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> Epd1in54c<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin + Wait,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayUs,
{
    async fn command(&mut self, spi: &mut SPI, command: Command) -> Result<(), SPI::Error> {
        self.interface.cmd(spi, command).await
    }

    async fn send_data(&mut self, spi: &mut SPI, data: &[u8]) -> Result<(), SPI::Error> {
        self.interface.data(spi, data).await
    }

    async fn cmd_with_data(
        &mut self,
        spi: &mut SPI,
        command: Command,
        data: &[u8],
    ) -> Result<(), SPI::Error> {
        self.interface.cmd_with_data(spi, command, data).await
    }

    async fn send_resolution(&mut self, spi: &mut SPI) -> Result<(), SPI::Error> {
        let w = self.width();
        let h = self.height();

        self.command(spi, Command::ResolutionSetting).await?;

        // | D7 | D6 | D5 | D4 | D3 | D2 | D1 | D0 |
        // |       HRES[7:3]        |  0 |  0 |  0 |
        self.send_data(spi, &[(w as u8) & 0b1111_1000]).await?;
        // | D7 | D6 | D5 | D4 | D3 | D2 | D1 |      D0 |
        // |  - |  - |  - |  - |  - |  - |  - | VRES[8] |
        self.send_data(spi, &[(w >> 8) as u8]).await?;
        // | D7 | D6 | D5 | D4 | D3 | D2 | D1 |      D0 |
        // |                  VRES[7:0]                 |
        // Specification shows C/D is zero while sending the last byte,
        // but upstream code does not implement it like that. So for now
        // we follow upstream code.
        self.send_data(spi, &[h as u8]).await
    }
}
