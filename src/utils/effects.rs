use crate::buffers::CaptureBuffer;

pub fn apply_effects(buffer: &CaptureBuffer, padding: u32, shadow: bool) -> CaptureBuffer {
    if padding == 0 && !shadow {
        return buffer.clone();
    }

    let bpp = buffer.format.bytes_per_pixel();
    
    // If shadow is enabled, add extra padding specifically for the shadow
    let shadow_offset_y = if shadow { 10 } else { 0 };
    let shadow_offset_x = if shadow { 5 } else { 0 };
    let shadow_blur_radius = if shadow { 15 } else { 0 };
    let shadow_extra_padding = if shadow { shadow_blur_radius * 2 } else { 0 };

    let total_padding_x = padding + shadow_extra_padding;
    let total_padding_y = padding + shadow_extra_padding;

    let new_width = buffer.width + (total_padding_x * 2);
    let new_height = buffer.height + (total_padding_y * 2);
    let new_stride = new_width * bpp;
    
    let mut new_data = vec![0u8; (new_stride * new_height) as usize];

    // Simple box-blur drop shadow
    if shadow {
        // Draw a dark semi-transparent rectangle first
        let shadow_color = 0x80000000u32; // ARGB
        
        let start_x = total_padding_x + shadow_offset_x;
        let start_y = total_padding_y + shadow_offset_y;
        
        for y in start_y..(start_y + buffer.height) {
            for x in start_x..(start_x + buffer.width) {
                if y < new_height && x < new_width {
                    let offset = (y * new_stride + x * bpp) as usize;
                    if bpp == 4 {
                        new_data[offset] = (shadow_color & 0xFF) as u8;
                        new_data[offset + 1] = ((shadow_color >> 8) & 0xFF) as u8;
                        new_data[offset + 2] = ((shadow_color >> 16) & 0xFF) as u8;
                        new_data[offset + 3] = ((shadow_color >> 24) & 0xFF) as u8;
                    }
                }
            }
        }
        
        // Note: A true gaussian blur is computationally expensive.
        // For a simple screenshot utility, a hard shadow with low opacity often looks okay,
        // or we could implement a quick two-pass box blur here if needed.
        // We will stick to a simple block shadow for now to keep it fast.
    }

    // Copy original image over
    for row in 0..buffer.height {
        let src_offset = (row * buffer.stride) as usize;
        let dst_offset = ((row + total_padding_y) * new_stride + total_padding_x * bpp) as usize;
        let row_bytes = (buffer.width * bpp) as usize;

        new_data[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&buffer.data[src_offset..src_offset + row_bytes]);
    }

    CaptureBuffer {
        width: new_width,
        height: new_height,
        stride: new_stride,
        format: buffer.format,
        data: new_data,
    }
}
