//! Saída gráfica RGB e terminal de texto VGA compatível, sem dependência de heap.
//! HDMI/DisplayPort são conectores do controlador gráfico; este módulo usa o
//! framebuffer já configurado por firmware/bootloader, sem programar o link.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_per_pixel: u8,
    pub red_size: u8,
    pub red_shift: u8,
    pub green_size: u8,
    pub green_shift: u8,
    pub blue_size: u8,
    pub blue_shift: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoError {
    InvalidDimensions,
    UnsupportedPixelFormat,
    InvalidPitch,
    BufferTooSmall,
    ArithmeticOverflow,
}

pub struct Framebuffer<'a> {
    bytes: &'a mut [u8],
    width: usize,
    height: usize,
    pitch: usize,
    format: PixelFormat,
}

impl<'a> Framebuffer<'a> {
    pub fn new(
        bytes: &'a mut [u8],
        width: usize,
        height: usize,
        pitch: usize,
        format: PixelFormat,
    ) -> Result<Self, VideoError> {
        if width == 0 || height == 0 {
            return Err(VideoError::InvalidDimensions);
        }
        if !matches!(format.bits_per_pixel, 24 | 32)
            || !valid_channel(format.red_size, format.red_shift, format.bits_per_pixel)
            || !valid_channel(format.green_size, format.green_shift, format.bits_per_pixel)
            || !valid_channel(format.blue_size, format.blue_shift, format.bits_per_pixel)
            || overlap(
                format.red_size,
                format.red_shift,
                format.green_size,
                format.green_shift,
            )
            || overlap(
                format.red_size,
                format.red_shift,
                format.blue_size,
                format.blue_shift,
            )
            || overlap(
                format.green_size,
                format.green_shift,
                format.blue_size,
                format.blue_shift,
            )
        {
            return Err(VideoError::UnsupportedPixelFormat);
        }
        let bytes_per_pixel = usize::from(format.bits_per_pixel / 8);
        let row = width
            .checked_mul(bytes_per_pixel)
            .ok_or(VideoError::ArithmeticOverflow)?;
        if pitch < row {
            return Err(VideoError::InvalidPitch);
        }
        let required = pitch
            .checked_mul(height)
            .ok_or(VideoError::ArithmeticOverflow)?;
        if bytes.len() < required {
            return Err(VideoError::BufferTooSmall);
        }
        Ok(Self {
            bytes,
            width,
            height,
            pitch,
            format,
        })
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn write_pixel(&mut self, x: usize, y: usize, rgb: [u8; 3]) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let bpp = usize::from(self.format.bits_per_pixel / 8);
        let Some(offset) = y.checked_mul(self.pitch).and_then(|row| {
            x.checked_mul(bpp)
                .and_then(|column| row.checked_add(column))
        }) else {
            return false;
        };
        let pixel = channel(rgb[0], self.format.red_size, self.format.red_shift)
            | channel(rgb[1], self.format.green_size, self.format.green_shift)
            | channel(rgb[2], self.format.blue_size, self.format.blue_shift);
        let encoded = pixel.to_le_bytes();
        self.bytes[offset..offset + bpp].copy_from_slice(&encoded[..bpp]);
        true
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, rgb: [u8; 3]) {
        let x_end = x.saturating_add(width).min(self.width);
        let y_end = y.saturating_add(height).min(self.height);
        for row in y.min(self.height)..y_end {
            for column in x.min(self.width)..x_end {
                self.write_pixel(column, row, rgb);
            }
        }
    }
}

/// Adapta o framebuffer validado ao ecossistema embedded-graphics.
/// A feature é opcional para manter o caminho mínimo do kernel sem dependências.
#[cfg(feature = "graphics")]
impl embedded_graphics::draw_target::DrawTarget for Framebuffer<'_> {
    type Color = embedded_graphics::pixelcolor::Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        use embedded_graphics::prelude::{Point, RgbColor};

        for embedded_graphics::Pixel(Point { x, y }, color) in pixels {
            if x >= 0 && y >= 0 {
                self.write_pixel(x as usize, y as usize, [color.r(), color.g(), color.b()]);
            }
        }
        Ok(())
    }
}

#[cfg(feature = "graphics")]
impl embedded_graphics::geometry::OriginDimensions for Framebuffer<'_> {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(self.width as u32, self.height as u32)
    }
}

fn valid_channel(size: u8, shift: u8, bpp: u8) -> bool {
    size > 0 && size <= 8 && shift < bpp && u16::from(size) + u16::from(shift) <= u16::from(bpp)
}

fn overlap(a_size: u8, a_shift: u8, b_size: u8, b_shift: u8) -> bool {
    let a = ((1u32 << a_size) - 1) << a_shift;
    let b = ((1u32 << b_size) - 1) << b_shift;
    a & b != 0
}

fn channel(value: u8, size: u8, shift: u8) -> u32 {
    let max = (1u32 << size) - 1;
    ((u32::from(value) * max / 255) << shift) & (max << shift)
}

/// Escreve texto ASCII num buffer VGA de células (caractere + atributo).
pub struct VgaTextConsole<'a> {
    cells: &'a mut [u16],
    columns: usize,
    rows: usize,
    column: usize,
    row: usize,
    attribute: u8,
}

impl<'a> VgaTextConsole<'a> {
    pub fn new(cells: &'a mut [u16], columns: usize, rows: usize, attribute: u8) -> Option<Self> {
        let count = columns.checked_mul(rows)?;
        if columns == 0 || rows == 0 || cells.len() < count {
            return None;
        }
        Some(Self {
            cells,
            columns,
            rows,
            column: 0,
            row: 0,
            attribute,
        })
    }

    pub fn clear(&mut self) {
        self.cells[..self.columns * self.rows]
            .fill((u16::from(self.attribute) << 8) | u16::from(b' '));
        self.column = 0;
        self.row = 0;
    }

    pub fn write_str(&mut self, text: &str) {
        for byte in text.bytes() {
            match byte {
                b'\n' => self.newline(),
                b'\r' => self.column = 0,
                0x20..=0x7e => self.write_byte(byte),
                _ => self.write_byte(b'?'),
            }
        }
    }

    fn write_byte(&mut self, byte: u8) {
        let index = self.row * self.columns + self.column;
        self.cells[index] = (u16::from(self.attribute) << 8) | u16::from(byte);
        self.column += 1;
        if self.column == self.columns {
            self.newline();
        }
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row == self.rows {
            self.cells
                .copy_within(self.columns..self.columns * self.rows, 0);
            let start = (self.rows - 1) * self.columns;
            self.cells[start..start + self.columns]
                .fill((u16::from(self.attribute) << 8) | u16::from(b' '));
            self.row = self.rows - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGBX: PixelFormat = PixelFormat {
        bits_per_pixel: 32,
        red_size: 8,
        red_shift: 16,
        green_size: 8,
        green_shift: 8,
        blue_size: 8,
        blue_shift: 0,
    };

    #[test]
    fn framebuffer_validates_pitch_and_writes_rgb_pixels() {
        let mut bytes = [0u8; 32];
        let mut fb = Framebuffer::new(&mut bytes, 2, 2, 16, RGBX).unwrap();
        assert!(fb.write_pixel(1, 1, [255, 128, 0]));
        assert_eq!(&fb.bytes[20..24], &[0, 128, 255, 0]);
        assert!(!fb.write_pixel(2, 0, [1, 2, 3]));
    }

    #[test]
    fn framebuffer_rejects_invalid_layouts_and_clips_rectangles() {
        assert_eq!(
            Framebuffer::new(&mut [0; 8], 2, 2, 4, RGBX).err(),
            Some(VideoError::InvalidPitch)
        );
        let mut bytes = [0u8; 16];
        let mut fb = Framebuffer::new(&mut bytes, 2, 2, 8, RGBX).unwrap();
        fb.fill_rect(1, 1, usize::MAX, usize::MAX, [0, 255, 0]);
        assert_eq!(&fb.bytes[12..16], &[0, 255, 0, 0]);
    }

    #[cfg(feature = "graphics")]
    #[test]
    fn embedded_graphics_draws_rgb_pixels_into_framebuffer() {
        use embedded_graphics::pixelcolor::Rgb888;
        use embedded_graphics::prelude::{DrawTarget, Point};

        let mut bytes = [0u8; 16];
        let mut fb = Framebuffer::new(&mut bytes, 2, 2, 8, RGBX).unwrap();
        fb.draw_iter([embedded_graphics::Pixel(
            Point::new(1, 0),
            Rgb888::new(12, 34, 56),
        )])
        .unwrap();
        assert_eq!(&fb.bytes[4..8], &[56, 34, 12, 0]);
    }

    #[test]
    fn vga_console_writes_ascii_and_scrolls() {
        let mut cells = [0u16; 6];
        let mut tty = VgaTextConsole::new(&mut cells, 3, 2, 0x1f).unwrap();
        tty.write_str("ABCDEFG");
        assert_eq!(tty.cells[0] & 0xff, b'D' as u16);
        assert_eq!(tty.cells[1] & 0xff, b'E' as u16);
        assert_eq!(tty.cells[2] & 0xff, b'F' as u16);
    }
}
