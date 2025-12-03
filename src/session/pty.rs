#[cfg(feature = "ratatui")]
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub width: u32,
    pub height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[cfg(feature = "ratatui")]
impl TryFrom<PtySize> for Rect {
    type Error = crate::Error;

    fn try_from(val: PtySize) -> Result<Self, Self::Error> {
        let width = u16::try_from(val.width)?;
        let height = u16::try_from(val.height)?;

        Ok(Self::new(0, 0, width, height))
    }
}
