//! A simple Driver for the Waveshare 2.9" E-Ink Display via SPI
//!
//!
//! # Example for the 2.9 in E-Ink Display
//!
//!```rust, no_run
//!# use embedded_hal_mock::eh1::*;
//!# fn main() -> Result<(), embedded_hal::spi::ErrorKind> {
//!use embedded_graphics::{
//!    pixelcolor::BinaryColor::On as Black, prelude::*, primitives::{Line, PrimitiveStyle},
//!};
//!use epd_waveshare::{epd2in9::*, prelude::*};
//!#
//!# let expectations = [];
//!# let mut spi = spi::Mock::new(&expectations);
//!# let expectations = [];
//!# let cs_pin = pin::Mock::new(&expectations);
//!# let busy_in = pin::Mock::new(&expectations);
//!# let dc = pin::Mock::new(&expectations);
//!# let rst = pin::Mock::new(&expectations);
//!# let mut delay = delay::NoopDelay::new();
//!
//!// Setup EPD
//!let mut epd = Epd2in9::new(&mut spi, busy_in, dc, rst, &mut None)?;
//!
//!// Use display graphics from embedded-graphics
//!let mut display = Display2in9::default();
//!
//!// Use embedded graphics for drawing a line
//!let _ = Line::new(Point::new(0, 120), Point::new(0, 295))
//!    .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
//!    .draw(&mut display);
//!
//!    // Display updated frame
//!epd.update_frame(&mut spi, &display.buffer(), &mut delay)?;
//!epd.display_frame(&mut spi, &mut delay)?;
//!
//!// Set the EPD to sleep
//!epd.sleep(&mut spi, &mut delay)?;
//!# Ok(())
//!# }
//!```

/// Width of epd2in9 in pixels
pub const WIDTH: u32 = 128;
/// Height of epd2in9 in pixels
pub const HEIGHT: u32 = 296;
/// Default Background Color (white)
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::White;
const IS_BUSY_LOW: bool = false;
const SINGLE_BYTE_WRITE: bool = true;

use core::fmt::{Debug, Display};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{digital::Wait, spi::SpiDevice};

use crate::{
    traits::{ErrorType, InternalWiAdditions, RefreshLut, WaveshareDisplay},
    type_a::{
        command::Command,
        constants::{LUT_FULL_UPDATE, LUT_PARTIAL_UPDATE},
    },
};

use crate::buffer_len;
use crate::color::Color;
use crate::error::ErrorKind;
use crate::interface::DisplayInterface;

/// Display with Fullsize buffer for use with the 2in9 EPD
#[cfg(feature = "graphics")]
pub type Display2in9 = crate::graphics::Display<
    WIDTH,
    HEIGHT,
    false,
    { buffer_len(WIDTH as usize, HEIGHT as usize) },
    Color,
>;

/// Epd2in9 driver
///
pub struct Epd2in9<SPI, BUSY, DC, RST> {
    /// SPI
    interface: DisplayInterface<SPI, BUSY, DC, RST, SINGLE_BYTE_WRITE>,
    /// Color
    background_color: Color,
    /// Refresh LUT
    refresh: RefreshLut,
}

impl<SPI, BUSY, DC, RST> ErrorType<SPI, BUSY, DC, RST> for Epd2in9<SPI, BUSY, DC, RST>
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

impl<SPI, BUSY, DC, RST> InternalWiAdditions<SPI, BUSY, DC, RST> for Epd2in9<SPI, BUSY, DC, RST>
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

        self.wait_until_idle(spi).await?;

        // 3 Databytes:
        // A[7:0]
        // 0.. A[8]
        // 0.. B[2:0]
        // Default Values: A = Height of Screen (0x127), B = 0x00 (GD, SM and TB=0?)
        self.interface
            .cmd_with_data(spi, Command::DriverOutputControl, &[0x27, 0x01, 0x00])
            .await?;

        // 3 Databytes: (and default values from datasheet and arduino)
        // 1 .. A[6:0]  = 0xCF | 0xD7
        // 1 .. B[6:0]  = 0xCE | 0xD6
        // 1 .. C[6:0]  = 0x8D | 0x9D
        //TODO: test
        self.interface
            .cmd_with_data(spi, Command::BoosterSoftStartControl, &[0xD7, 0xD6, 0x9D])
            .await?;

        // One Databyte with value 0xA8 for 7V VCOM
        self.interface
            .cmd_with_data(spi, Command::WriteVcomRegister, &[0xA8])
            .await?;

        // One Databyte with default value 0x1A for 4 dummy lines per gate
        self.interface
            .cmd_with_data(spi, Command::SetDummyLinePeriod, &[0x1A])
            .await?;

        // One Databyte with default value 0x08 for 2us per line
        self.interface
            .cmd_with_data(spi, Command::SetGateLineWidth, &[0x08])
            .await?;

        // One Databyte with default value 0x03
        //  -> address: x increment, y increment, address counter is updated in x direction
        self.interface
            .cmd_with_data(spi, Command::DataEntryModeSetting, &[0x03])
            .await?;

        self.set_lut(spi, None).await
    }
}

impl<SPI, BUSY, DC, RST> WaveshareDisplay<SPI, BUSY, DC, RST> for Epd2in9<SPI, BUSY, DC, RST>
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
    fn width(&self) -> u32 {
        WIDTH
    }

    fn height(&self) -> u32 {
        HEIGHT
    }

    async fn new(
        spi: &mut SPI,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay_us: Option<u32>,
    ) -> Result<Self, Self::Error> {
        let interface = DisplayInterface::new(busy, dc, rst, delay_us);

        let mut epd = Epd2in9 {
            interface,
            background_color: DEFAULT_BACKGROUND_COLOR,
            refresh: RefreshLut::Full,
        };

        epd.init(spi).await?;

        Ok(epd)
    }

    async fn sleep(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        // 0x00 for Normal mode (Power on Reset), 0x01 for Deep Sleep Mode
        //TODO: is 0x00 needed here? (see also epd1in54)
        self.interface
            .cmd_with_data(spi, Command::DeepSleepMode, &[0x00])
            .await?;
        Ok(())
    }

    async fn wake_up(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.init(spi).await?;
        Ok(())
    }

    async fn update_frame(&mut self, spi: &mut SPI, buffer: &[u8]) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.use_full_frame(spi).await?;

        self.interface
            .cmd_with_data(spi, Command::WriteRam, buffer)
            .await?;
        Ok(())
    }

    //TODO: update description: last 3 bits will be ignored for width and x_pos
    async fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.set_ram_area(spi, x, y, x + width, y + height).await?;
        self.set_ram_counter(spi, x, y).await?;

        self.interface
            .cmd_with_data(spi, Command::WriteRam, buffer)
            .await?;
        Ok(())
    }

    async fn display_frame(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        // enable clock signal, enable cp, display pattern -> 0xC4 (tested with the arduino version)
        //TODO: test control_1 or control_2 with default value 0xFF (from the datasheet)
        self.interface
            .cmd_with_data(spi, Command::DisplayUpdateControl2, &[0xC4])
            .await?;

        self.interface.cmd(spi, Command::MasterActivation).await?;
        // MASTER Activation should not be interupted to avoid currption of panel images
        // therefore a terminate command is send
        self.interface.cmd(spi, Command::Nop).await?;
        Ok(())
    }

    async fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
    ) -> Result<(), Self::Error> {
        self.update_frame(spi, buffer).await?;
        self.display_frame(spi).await?;
        Ok(())
    }

    async fn clear_frame(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.wait_until_idle(spi).await?;
        self.use_full_frame(spi).await?;

        // clear the ram with the background color
        let color = self.background_color.get_byte_value();

        self.interface.cmd(spi, Command::WriteRam).await?;
        self.interface
            .data_x_times(spi, color, WIDTH / 8 * HEIGHT)
            .await?;
        Ok(())
    }

    fn set_background_color(&mut self, background_color: Color) {
        self.background_color = background_color;
    }

    fn background_color(&self) -> &Color {
        &self.background_color
    }

    async fn set_lut(
        &mut self,
        spi: &mut SPI,
        refresh_rate: Option<RefreshLut>,
    ) -> Result<(), Self::Error> {
        if let Some(refresh_lut) = refresh_rate {
            self.refresh = refresh_lut;
        }
        match self.refresh {
            RefreshLut::Full => self.set_lut_helper(spi, &LUT_FULL_UPDATE).await,
            RefreshLut::Quick => self.set_lut_helper(spi, &LUT_PARTIAL_UPDATE).await,
        }
    }

    async fn wait_until_idle(&mut self, spi: &mut SPI) -> Result<(), Self::Error> {
        self.interface.wait_until_idle(spi, IS_BUSY_LOW).await
    }
}

impl<SPI, BUSY, DC, RST> Epd2in9<SPI, BUSY, DC, RST>
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
    async fn use_full_frame(
        &mut self,
        spi: &mut SPI,
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        // choose full frame/ram
        self.set_ram_area(spi, 0, 0, WIDTH - 1, HEIGHT - 1).await?;

        // start from the beginning
        self.set_ram_counter(spi, 0, 0).await
    }

    async fn set_ram_area(
        &mut self,
        spi: &mut SPI,
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        assert!(start_x < end_x);
        assert!(start_y < end_y);

        // x is positioned in bytes, so the last 3 bits which show the position inside a byte in the ram
        // aren't relevant
        self.interface
            .cmd_with_data(
                spi,
                Command::SetRamXAddressStartEndPosition,
                &[(start_x >> 3) as u8, (end_x >> 3) as u8],
            )
            .await?;

        // 2 Databytes: A[7:0] & 0..A[8] for each - start and end
        self.interface
            .cmd_with_data(
                spi,
                Command::SetRamYAddressStartEndPosition,
                &[
                    start_y as u8,
                    (start_y >> 8) as u8,
                    end_y as u8,
                    (end_y >> 8) as u8,
                ],
            )
            .await
    }

    async fn set_ram_counter(
        &mut self,
        spi: &mut SPI,
        x: u32,
        y: u32,
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        self.wait_until_idle(spi).await?;
        // x is positioned in bytes, so the last 3 bits which show the position inside a byte in the ram
        // aren't relevant
        self.interface
            .cmd_with_data(spi, Command::SetRamXAddressCounter, &[(x >> 3) as u8])
            .await?;

        // 2 Databytes: A[7:0] & 0..A[8]
        self.interface
            .cmd_with_data(
                spi,
                Command::SetRamYAddressCounter,
                &[y as u8, (y >> 8) as u8],
            )
            .await?;
        Ok(())
    }

    /// Set your own LUT, this function is also used internally for set_lut
    async fn set_lut_helper(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
    ) -> Result<(), <Self as ErrorType<SPI, BUSY, DC, RST>>::Error> {
        self.wait_until_idle(spi).await?;
        assert!(buffer.len() == 30);
        self.interface
            .cmd_with_data(spi, Command::WriteLutRegister, buffer)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epd_size() {
        assert_eq!(WIDTH, 128);
        assert_eq!(HEIGHT, 296);
        assert_eq!(DEFAULT_BACKGROUND_COLOR, Color::White);
    }
}
