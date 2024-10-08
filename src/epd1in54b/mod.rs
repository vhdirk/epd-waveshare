//! A simple Driver for the Waveshare 1.54" (B) E-Ink Display via SPI
use core::fmt::{Debug, Display};
use embedded_hal::{
    delay::*,
    digital::{InputPin, OutputPin},
};
use embedded_hal_async::{digital::Wait, spi::SpiDevice};

use crate::error::ErrorKind;
use crate::interface::DisplayInterface;
use crate::traits::{
    ErrorType, InternalWiAdditions, RefreshLut, WaveshareDisplay, WaveshareThreeColorDisplay,
};

//The Lookup Tables for the Display
mod constants;
use crate::epd1in54b::constants::*;

/// Width of epd1in54 in pixels
pub const WIDTH: u32 = 200;
/// Height of epd1in54 in pixels
pub const HEIGHT: u32 = 200;
/// Default Background Color (white)
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::White;
const IS_BUSY_LOW: bool = true;
const SINGLE_BYTE_WRITE: bool = true;

use crate::color::Color;

pub(crate) mod command;
use self::command::Command;
use crate::buffer_len;

/// Full size buffer for use with the 1in54b EPD
/// TODO this should be a TriColor, but let's keep it as is at first
#[cfg(feature = "graphics")]
pub type Display1in54b = crate::graphics::Display<
    WIDTH,
    HEIGHT,
    false,
    { buffer_len(WIDTH as usize, HEIGHT as usize) },
    Color,
>;

/// Epd1in54b driver
pub struct Epd1in54b<SPI, BUSY, DC, RST> {
    interface: DisplayInterface<SPI, BUSY, DC, RST, SINGLE_BYTE_WRITE>,
    color: Color,
}

impl<SPI, BUSY, DC, RST> ErrorType<SPI, BUSY, DC, RST> for Epd1in54b<SPI, BUSY, DC, RST>
where
    SPI: SpiDevice,
    SPI::Error: Copy + Debug + Display,
    BUSY: InputPin + Wait,
    BUSY::Error: Copy + Debug + Display,
    DC: OutputPin,
    DC::Error: Copy + Debug + Display,
    RST: OutputPin,
    RST::Error: Copy + Debug + Display,
{
    type Error = ErrorKind<SPI, BUSY, DC, RST>;
}

impl<SPI, BUSY, DC, RST> InternalWiAdditions<SPI, BUSY, DC, RST> for Epd1in54b<SPI, BUSY, DC, RST>
where
    SPI: SpiDevice,
    SPI::Error: Copy + Debug + Display,
    BUSY: InputPin + Wait,
    BUSY::Error: Copy + Debug + Display,
    DC: OutputPin,
    DC::Error: Copy + Debug + Display,
    RST: OutputPin,
    RST::Error: Copy + Debug + Display,
{
    async fn init(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.interface.reset(spi, 10_000, 10_000).await?;

        // set the power settings
        self.interface
            .cmd_with_data(spi, Command::PowerSetting, &[0x07, 0x00, 0x08, 0x00])
            .await?;

        // start the booster
        self.interface
            .cmd_with_data(spi, Command::BoosterSoftStart, &[0x07, 0x07, 0x07])
            .await?;

        // power on
        self.command(spi, Command::PowerOn).await?;
        self.interface.delay(spi, 5000).await?;
        self.wait_until_idle(spi).await?;

        // set the panel settings
        self.cmd_with_data(spi, Command::PanelSetting, &[0xCF])
            .await?;

        self.cmd_with_data(spi, Command::VcomAndDataIntervalSetting, &[0x37])
            .await?;

        // PLL
        self.cmd_with_data(spi, Command::PllControl, &[0x39])
            .await?;

        // set resolution
        self.send_resolution(spi).await?;

        self.cmd_with_data(spi, Command::VcmDcSetting, &[0x0E])
            .await?;

        self.set_lut(spi, None).await?;

        self.wait_until_idle(spi).await?;

        Ok(())
    }
}

impl<SPI, BUSY, DC, RST> WaveshareThreeColorDisplay<SPI, BUSY, DC, RST>
    for Epd1in54b<SPI, BUSY, DC, RST>
where
    SPI: SpiDevice,
    SPI::Error: Copy + Debug + Display,
    BUSY: InputPin + Wait,
    BUSY::Error: Copy + Debug + Display,
    DC: OutputPin,
    DC::Error: Copy + Debug + Display,
    RST: OutputPin,
    RST::Error: Copy + Debug + Display,
{
    async fn update_color_frame(
        &mut self,
        spi: &mut SPI,
        black: &[u8],
        chromatic: &[u8],
    ) -> Result<(), Self::Error> {
        self.update_achromatic_frame(spi, black).await?;
        self.update_chromatic_frame(spi, chromatic).await
    }

    async fn update_achromatic_frame(
        &mut self,
        spi: &mut SPI,
        black: &[u8],
    ) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.send_resolution(spi).await?;

        self.interface
            .cmd(spi, Command::DataStartTransmission1)
            .await?;

        for b in black {
            let expanded = expand_bits(*b);
            self.interface.data(spi, &expanded).await?;
        }
        Ok(())
    }

    async fn update_chromatic_frame(
        &mut self,
        spi: &mut SPI,
        chromatic: &[u8],
    ) -> Result<(), Self::Error> {
        self.interface
            .cmd(spi, Command::DataStartTransmission2)
            .await?;
        self.interface.data(spi, chromatic).await?;
        Ok(())
    }
}

impl<SPI, BUSY, DC, RST> WaveshareDisplay<SPI, BUSY, DC, RST> for Epd1in54b<SPI, BUSY, DC, RST>
where
    SPI: SpiDevice,
    SPI::Error: Copy + Debug + Display,
    BUSY: InputPin + Wait,
    BUSY::Error: Copy + Debug + Display,
    DC: OutputPin,
    DC::Error: Copy + Debug + Display,
    RST: OutputPin,
    RST::Error: Copy + Debug + Display,
{
    type DisplayColor = Color;
    async fn new(
        spi: &mut SPI,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay_us: Option<u32>,
    ) -> Result<Self, Self::Error> {
        let interface = DisplayInterface::new(busy, dc, rst, delay_us);
        let color = DEFAULT_BACKGROUND_COLOR;

        let mut epd = Epd1in54b { interface, color };

        epd.init(spi).await?;

        Ok(epd)
    }

    async fn sleep(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.interface
            .cmd_with_data(spi, Command::VcomAndDataIntervalSetting, &[0x17])
            .await?; //border floating

        self.interface
            .cmd_with_data(spi, Command::VcmDcSetting, &[0x00])
            .await?; // Vcom to 0V

        self.interface
            .cmd_with_data(spi, Command::PowerSetting, &[0x02, 0x00, 0x00, 0x00])
            .await?; //VG&VS to 0V fast

        self.wait_until_idle(spi).await?;

        //NOTE: The example code has a 1s delay here

        self.command(spi, Command::PowerOff).await
    }

    async fn wake_up(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.init(spi).await
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

    async fn update_frame(&mut self, spi: &mut SPI, buffer: &[u8]) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.send_resolution(spi).await?;

        self.interface
            .cmd(spi, Command::DataStartTransmission1)
            .await?;

        for b in buffer {
            // Two bits per pixel
            let expanded = expand_bits(*b);
            self.interface.data(spi, &expanded).await?;
        }

        //NOTE: Example code has a delay here

        // Clear the read layer
        let color = self.color.get_byte_value();
        let nbits = WIDTH * (HEIGHT / 8);

        self.interface
            .cmd(spi, Command::DataStartTransmission2)
            .await?;
        self.interface.data_x_times(spi, color, nbits).await

        //NOTE: Example code has a delay here
    }

    #[allow(unused)]
    async fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), Self::Error> {
        unimplemented!()
    }

    async fn display_frame(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.command(spi, Command::DisplayRefresh).await
    }

    async fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
    ) -> Result<(), Self::Error> {
        self.update_frame(spi, buffer).await?;
        self.display_frame(spi).await
    }

    async fn clear_frame(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.send_resolution(spi).await?;

        let color = DEFAULT_BACKGROUND_COLOR.get_byte_value();

        // Clear the black
        self.interface
            .cmd(spi, Command::DataStartTransmission1)
            .await?;

        // Uses 2 bits per pixel
        self.interface
            .data_x_times(spi, color, 2 * (WIDTH / 8 * HEIGHT))
            .await?;

        // Clear the red
        self.interface
            .data_x_times(spi, color, WIDTH / 8 * HEIGHT)
            .await?;
        Ok(())
    }

    async fn set_lut(
        &mut self,
        spi: &mut SPI,
        _refresh_rate: Option<RefreshLut>,
    ) -> Result<(), Self::Error> {
        self.interface
            .cmd_with_data(spi, Command::LutForVcom, LUT_VCOM0)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutWhiteToWhite, LUT_WHITE_TO_WHITE)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutBlackToWhite, LUT_BLACK_TO_WHITE)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutG0, LUT_G1)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutG1, LUT_G2)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutRedVcom, LUT_RED_VCOM)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutRed0, LUT_RED0)
            .await?;
        self.interface
            .cmd_with_data(spi, Command::LutRed1, LUT_RED1)
            .await
    }

    async fn wait_until_idle(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.interface.wait_until_idle(spi, IS_BUSY_LOW).await
    }
}

impl<SPI, BUSY, DC, RST> Epd1in54b<SPI, BUSY, DC, RST>
where
    SPI: SpiDevice,
    SPI::Error: Copy + Debug + Display,
    BUSY: InputPin + Wait,
    BUSY::Error: Copy + Debug + Display,
    DC: OutputPin,
    DC::Error: Copy + Debug + Display,
    RST: OutputPin,
    RST::Error: Copy + Debug + Display,
{
    async fn command(
        &mut self,
        spi: &mut SPI,
        command: Command,
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        self.interface.cmd(spi, command).await
    }

    async fn send_data(
        &mut self,
        spi: &mut SPI,
        data: &[u8],
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        self.interface.data(spi, data).await
    }

    async fn cmd_with_data(
        &mut self,
        spi: &mut SPI,
        command: Command,
        data: &[u8],
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        self.interface.cmd_with_data(spi, command, data).await
    }

    async fn send_resolution(
        &mut self,
        spi: &mut SPI,
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        let w = self.width();
        let h = self.height();

        self.command(spi, Command::ResolutionSetting).await?;

        self.send_data(spi, &[w as u8]).await?;
        self.send_data(spi, &[(h >> 8) as u8]).await?;
        self.send_data(spi, &[h as u8]).await
    }
}

fn expand_bits(bits: u8) -> [u8; 2] {
    let mut x = bits as u16;

    x = (x | (x << 4)) & 0x0F0F;
    x = (x | (x << 2)) & 0x3333;
    x = (x | (x << 1)) & 0x5555;
    x = x | (x << 1);

    [(x >> 8) as u8, (x & 0xFF) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epd_size() {
        assert_eq!(WIDTH, 200);
        assert_eq!(HEIGHT, 200);
        assert_eq!(DEFAULT_BACKGROUND_COLOR, Color::White);
    }
}
