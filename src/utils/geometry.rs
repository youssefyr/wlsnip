use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};

/// A rectangular region on screen.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    /// Parse a region from slurp output format: "X,Y WxH"
    pub fn from_slurp(s: &str) -> Result<Self> {
        // slurp outputs: "X,Y WxH"
        let s = s.trim();
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(WlsnipError::RegionSelection(
                format!("invalid slurp geometry: {s:?}")
            ));
        }

        let pos: Vec<&str> = parts[0].split(',').collect();
        let dim: Vec<&str> = parts[1].split('x').collect();

        if pos.len() != 2 || dim.len() != 2 {
            return Err(WlsnipError::RegionSelection(
                format!("invalid slurp geometry: {s:?}")
            ));
        }

        let x: i32 = pos[0].parse().map_err(|_| {
            WlsnipError::RegionSelection(format!("invalid x coordinate: {:?}", pos[0]))
        })?;
        let y: i32 = pos[1].parse().map_err(|_| {
            WlsnipError::RegionSelection(format!("invalid y coordinate: {:?}", pos[1]))
        })?;
        let width: u32 = dim[0].parse().map_err(|_| {
            WlsnipError::RegionSelection(format!("invalid width: {:?}", dim[0]))
        })?;
        let height: u32 = dim[1].parse().map_err(|_| {
            WlsnipError::RegionSelection(format!("invalid height: {:?}", dim[1]))
        })?;

        Ok(Self { x, y, width, height })
    }
}

/// Crop a capture buffer to the specified region.
///
/// Returns a new buffer containing only the cropped area.
#[allow(dead_code)]
pub fn crop(buffer: &CaptureBuffer, region: &Region) -> Result<CaptureBuffer> {
    let bpp = buffer.format.bytes_per_pixel();

    // Validate bounds
    let src_x = region.x.max(0) as u32;
    let src_y = region.y.max(0) as u32;

    if src_x + region.width > buffer.width || src_y + region.height > buffer.height {
        return Err(WlsnipError::InvalidArg(format!(
            "crop region ({},{} {}x{}) exceeds buffer ({}x{})",
            region.x, region.y, region.width, region.height,
            buffer.width, buffer.height
        )));
    }

    let dst_stride = region.width * bpp;
    let mut dst_data = vec![0u8; (dst_stride * region.height) as usize];

    // Row-by-row copy
    for row in 0..region.height {
        let src_offset = ((src_y + row) * buffer.stride + src_x * bpp) as usize;
        let dst_offset = (row * dst_stride) as usize;
        let row_bytes = (region.width * bpp) as usize;

        dst_data[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&buffer.data[src_offset..src_offset + row_bytes]);
    }

    Ok(CaptureBuffer {
        width: region.width,
        height: region.height,
        stride: dst_stride,
        format: buffer.format,
        data: dst_data,
    })
}

/// Run `slurp` to let the user select a screen region interactively.
pub fn select_region_with_slurp(selection_color: Option<&str>) -> Result<Region> {
    let color = selection_color.unwrap_or("#fb751bff");
    let mut cmd = std::process::Command::new("slurp");
    cmd.args([
        "-b", "#2E2A1E55",
        "-c", color,
        "-s", "#fb751b22",
        "-w", "2",
    ]);
    
    let output = cmd.output()
        .map_err(|e| WlsnipError::RegionSelection(
            format!("failed to run slurp (is it installed?): {e}")
        ))?;

    if !output.status.success() {
        return Err(WlsnipError::RegionSelection(
            "slurp was cancelled or failed".to_string()
        ));
    }

    let geo = String::from_utf8_lossy(&output.stdout);
    Region::from_slurp(&geo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffers::PixelFormat;

    #[test]
    fn test_parse_slurp_geometry() {
        let region = Region::from_slurp("100,200 300x400").unwrap();
        assert_eq!(region.x, 100);
        assert_eq!(region.y, 200);
        assert_eq!(region.width, 300);
        assert_eq!(region.height, 400);
    }

    #[test]
    fn test_crop_basic() {
        let buffer = CaptureBuffer {
            width: 10,
            height: 10,
            stride: 40, // 10 * 4 bpp
            format: PixelFormat::Argb8888,
            data: vec![0xAA; 400], // 10*10*4
        };
        let region = Region { x: 2, y: 2, width: 5, height: 5 };
        let cropped = crop(&buffer, &region).unwrap();
        assert_eq!(cropped.width, 5);
        assert_eq!(cropped.height, 5);
        assert_eq!(cropped.data.len(), (5 * 5 * 4) as usize);
    }

    #[test]
    fn test_crop_out_of_bounds() {
        let buffer = CaptureBuffer {
            width: 10,
            height: 10,
            stride: 40,
            format: PixelFormat::Argb8888,
            data: vec![0; 400],
        };
        let region = Region { x: 5, y: 5, width: 10, height: 10 };
        assert!(crop(&buffer, &region).is_err());
    }
}
