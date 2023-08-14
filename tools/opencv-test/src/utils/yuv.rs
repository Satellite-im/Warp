// shamelessly stolen from here: https://github.com/hanguk0726/Avatar-Vision/blob/main/rust/src/tools/image_processing.rs

use opencv::{
    core::CV_32FC3,
    prelude::{Mat, MatTrait, MatTraitConst},
};
#[cfg(feature = "all")]
use openh264::formats::YUVSource;

pub const Y_SCALE: [[f32; 3]; 3] = [
    [0.2578125, 0.50390625, 0.09765625],
    [0.299, 0.587, 0.114],
    [0.183, 0.614, 0.062],
];
pub const Y_OFFSET: [f32; 3] = [16.0, 0.0, 16.0];

pub const U_SCALE: [[f32; 3]; 3] = [
    [-0.1484375, -0.2890625, 0.4375],
    [-0.169, -0.331, 0.500],
    [-0.101, -0.339, 0.439],
];
const U_OFFSET: [f32; 3] = [128.0, 128.0, 128.0];

pub const V_SCALE: [[f32; 3]; 3] = [
    [0.4375, -0.3671875, -0.0703125],
    [0.500, -0.419, -0.081],
    [0.439, -0.399, -0.040],
];
const V_OFFSET: [f32; 3] = [128.0, 128.0, 128.0];

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ColorScale {
    // coeffecients taken from https://github.com/hanguk0726/Avatar-Vision/blob/main/rust/src/tools/image_processing.rs
    Av,
    // full scale: https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
    Full,
    // HdTv scale: // https://web.archive.org/web/20180423091842/http://www.equasys.de/colorconversion.html
    HdTv,
}

impl ColorScale {
    pub fn to_idx(self) -> usize {
        match self {
            ColorScale::Av => 0,
            ColorScale::Full => 1,
            ColorScale::HdTv => 2,
        }
    }
}

fn rgb_to_yuv(rgb: (f32, f32, f32), color_scale: ColorScale) -> (u8, u8, u8) {
    let idx = color_scale.to_idx();

    let y_scale: &[f32; 3] = &Y_SCALE[idx];
    let u_scale: &[f32; 3] = &U_SCALE[idx];
    let v_scale: &[f32; 3] = &V_SCALE[idx];

    let y_offset = Y_OFFSET[idx];
    let u_offset = U_OFFSET[idx];
    let v_offset = V_OFFSET[idx];

    let y = y_scale[0] * rgb.0 + y_scale[1] * rgb.1 + y_scale[2] * rgb.2 + y_offset;
    let u = u_scale[0] * rgb.0 + u_scale[1] * rgb.1 + u_scale[2] * rgb.2 + u_offset;
    let v = v_scale[0] * rgb.0 + v_scale[1] * rgb.1 + v_scale[2] * rgb.2 + v_offset;

    (y as u8, u as u8, v as u8)
}

pub fn bgr_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(data.len() * 2);
    for chunk in data.chunks_exact(3) {
        rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], 255]);
    }
    rgba
}

pub fn yuyv422_to_rgb_(data: &[u8], rgba: bool) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(data.len() * 2);
    for chunk in data.chunks_exact(4) {
        let y0 = chunk[0] as f32;
        let u = chunk[1] as f32;
        let y1 = chunk[2] as f32;
        let v: f32 = chunk[3] as f32;

        let r0 = y0 + 1.370705 * (v - 128.);
        let g0 = y0 - 0.698001 * (v - 128.) - 0.337633 * (u - 128.);
        let b0 = y0 + 1.732446 * (u - 128.);

        let r1 = y1 + 1.370705 * (v - 128.);
        let g1 = y1 - 0.698001 * (v - 128.) - 0.337633 * (u - 128.);
        let b1 = y1 + 1.732446 * (u - 128.);

        if rgba {
            rgb.extend_from_slice(&[
                r0 as u8, g0 as u8, b0 as u8, 255, r1 as u8, g1 as u8, b1 as u8, 255,
            ]);
        } else {
            rgb.extend_from_slice(&[r0 as u8, g0 as u8, b0 as u8, r1 as u8, g1 as u8, b1 as u8]);
        }
    }
    rgb
}

pub fn bgr_to_yuv444(rgb: &[u8], width: usize, height: usize) -> Vec<u8> {
    let size = width * height * 3;
    let mut yuv = Vec::new();
    yuv.resize(size, 0_u8);
    let y_base = 0;
    let u_base = width * height;
    let v_base = u_base + u_base;

    for (idx, chunk) in rgb.chunks_exact(3).enumerate() {
        let rgb = (chunk[2] as f32, chunk[1] as f32, chunk[0] as f32);
        let (y, u, v) = rgb_to_yuv(rgb, ColorScale::Av);
        yuv[y_base + idx] = y;
        yuv[u_base + idx] = u;
        yuv[v_base + idx] = v;
    }

    yuv
}
// attempts to avoid the loss when converting from BGR to YUV420 by quadrupling the size of the output. this ensures no UV samples are discarded/averaged
pub fn bgr_to_yuv420(s: &[u8], width: usize, height: usize, color_scale: ColorScale) -> Vec<u8> {
    // for y
    let y_rc_2_idx = |row: usize, col: usize| (row * width * 2) + col;

    let yuv_len = (width * height) * 6;
    let mut yuv: Vec<u8> = Vec::new();
    yuv.resize(yuv_len, 0);
    let u_base = (width * height) * 4;
    let v_base = u_base + (width * height);
    let mut uv_idx = 0;
    for row in 0..height {
        for col in 0..width {
            let base_pos = (col + row * width) * 3;
            let b = s[base_pos];
            let g = s[base_pos + 1];
            let r = s[base_pos + 2];

            let rgb = (r as _, g as _, b as _);
            let (y, u, v) = rgb_to_yuv(rgb, color_scale);

            // each byte in the u/v plane corresponds to a 4x4 square on the y plane
            let y_row = row * 2;
            let y_col = col * 2;

            let idx = y_rc_2_idx(y_row, y_col);
            yuv[idx] = y;
            yuv[idx + 1] = y;
            let idx = y_rc_2_idx(y_row + 1, y_col);
            yuv[idx] = y;
            yuv[idx + 1] = y;

            yuv[u_base + uv_idx] = u;
            yuv[v_base + uv_idx] = v;
            uv_idx += 1;
        }
    }

    yuv
}

/// use opencv matrix transformation to convert from BGR to YUV
/// appears to be slower. hilarious.
pub fn bgr_to_yuv420_lossy_faster(
    frame: Mat,
    m: &Mat,
    width: usize,
    height: usize,
    color_scale: ColorScale,
) -> Vec<u8> {
    let len = width * height * 3;

    // the frame is in u8 format. can't very well multiply by a negative number and expect it to work.
    // instead have to convert to a float first.
    let mut frame2 = Mat::default();
    frame
        .convert_to(&mut frame2, CV_32FC3, 1.0, 0.0)
        .expect("failed to convert");

    let color_scale_idx = color_scale.to_idx();

    let offsets = [
        Y_OFFSET[color_scale_idx],
        U_OFFSET[color_scale_idx],
        V_OFFSET[color_scale_idx],
    ];
    let mut xformed = Mat::default();
    opencv::core::transform(&frame2, &mut xformed, &m).expect("failed to transform matrix");

    let p = xformed.data_mut() as *mut f32;
    let s = std::ptr::slice_from_raw_parts_mut(p, len as _);
    let s: &mut [f32] = unsafe { &mut *s };

    //////////////////

    let size = (3 * width * height) / 2;
    let mut yuv = Vec::new();
    yuv.resize(size, 0_u8);

    let u_base = width * height;
    let v_base = u_base + u_base / 4;
    let half_width = width / 2;

    // y is full size, u, v is quarter size
    let get_yuv = |x: usize, y: usize| -> (u8, u8, u8) {
        // two dim to single dim
        let base_pos = (x + y * width) * 3;
        let yuv = (
            s[base_pos] + offsets[0],
            s[base_pos + 1] + offsets[1],
            s[base_pos + 2] + offsets[2],
        );
        (yuv.0 as u8, yuv.1 as u8, yuv.2 as u8)
    };

    let write_y = |yuv: &mut [u8], x: usize, y: usize, y_| {
        yuv[x + y * width] = y_;
    };

    let write_u = |yuv: &mut [u8], x: usize, y: usize, u_| {
        yuv[u_base + x + y * half_width] = u_;
    };
    let write_v = |yuv: &mut [u8], x: usize, y: usize, v_| yuv[v_base + x + y * half_width] = v_;

    // todo: use rayon to do this in parallel?
    for i in 0..width / 2 {
        for j in 0..height / 2 {
            let px = i * 2;
            let py = j * 2;
            let pix0x0 = get_yuv(px, py);
            let pix0x1 = get_yuv(px, py + 1);
            let pix1x0 = get_yuv(px + 1, py);
            let pix1x1 = get_yuv(px + 1, py + 1);
            let avg_pix = (
                (pix0x0.0 as u32 + pix0x1.0 as u32 + pix1x0.0 as u32 + pix1x1.0 as u32) as f32
                    / 4.0,
                (pix0x0.1 as u32 + pix0x1.1 as u32 + pix1x0.1 as u32 + pix1x1.1 as u32) as f32
                    / 4.0,
                (pix0x0.2 as u32 + pix0x1.2 as u32 + pix1x0.2 as u32 + pix1x1.2 as u32) as f32
                    / 4.0,
            );
            write_y(&mut yuv[..], px, py, pix0x0.0);
            write_y(&mut yuv[..], px, py + 1, pix0x1.0);
            write_y(&mut yuv[..], px + 1, py, pix1x0.0);
            write_y(&mut yuv[..], px + 1, py + 1, pix1x1.0);
            write_u(&mut yuv[..], i, j, avg_pix.1 as u8);
            write_v(&mut yuv[..], i, j, avg_pix.2 as u8);
        }
    }
    yuv
}

// u and v are calculated by averaging a 4-pixel square
pub fn bgr_to_yuv420_lossy(
    rgb: &[u8],
    width: usize,
    height: usize,
    color_scale: ColorScale,
) -> Vec<u8> {
    let size = (3 * width * height) / 2;
    let mut yuv = vec![0; size];

    let u_base = width * height;
    let v_base = u_base + u_base / 4;
    let half_width = width / 2;

    // y is full size, u, v is quarter size
    let pixel = |x: usize, y: usize| -> (f32, f32, f32) {
        // two dim to single dim
        let base_pos = (x + y * width) * 3;
        (
            rgb[base_pos + 2] as f32,
            rgb[base_pos + 1] as f32,
            rgb[base_pos] as f32,
        )
    };

    let color_scale_idx = color_scale.to_idx();
    let y_scale: &[f32; 3] = &Y_SCALE[color_scale_idx];
    let u_scale: &[f32; 3] = &U_SCALE[color_scale_idx];
    let v_scale: &[f32; 3] = &V_SCALE[color_scale_idx];

    let y_offset = Y_OFFSET[color_scale_idx];
    let u_offset = U_OFFSET[color_scale_idx];
    let v_offset = V_OFFSET[color_scale_idx];

    let write_y = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[x + y * width] =
            (y_scale[0] * rgb.0 + y_scale[1] * rgb.1 + y_scale[2] * rgb.2 + y_offset) as u8;
    };

    let write_u = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[u_base + x + y * half_width] =
            (u_scale[0] * rgb.0 + u_scale[1] * rgb.1 + u_scale[2] * rgb.2 + u_offset) as u8;
    };

    let write_v = |yuv: &mut [u8], x: usize, y: usize, rgb: (f32, f32, f32)| {
        yuv[v_base + x + y * half_width] =
            (v_scale[0] * rgb.0 + v_scale[1] * rgb.1 + v_scale[2] * rgb.2 + v_offset) as u8;
    };
    for i in 0..width / 2 {
        for j in 0..height / 2 {
            let px = i * 2;
            let py = j * 2;
            let pix0x0 = pixel(px, py);
            let pix0x1 = pixel(px, py + 1);
            let pix1x0 = pixel(px + 1, py);
            let pix1x1 = pixel(px + 1, py + 1);
            let avg_pix = (
                (pix0x0.0 as u32 + pix0x1.0 as u32 + pix1x0.0 as u32 + pix1x1.0 as u32) as f32
                    / 4.0,
                (pix0x0.1 as u32 + pix0x1.1 as u32 + pix1x0.1 as u32 + pix1x1.1 as u32) as f32
                    / 4.0,
                (pix0x0.2 as u32 + pix0x1.2 as u32 + pix1x0.2 as u32 + pix1x1.2 as u32) as f32
                    / 4.0,
            );
            write_y(&mut yuv[..], px, py, pix0x0);
            write_y(&mut yuv[..], px, py + 1, pix0x1);
            write_y(&mut yuv[..], px + 1, py, pix1x0);
            write_y(&mut yuv[..], px + 1, py + 1, pix1x1);
            write_u(&mut yuv[..], i, j, avg_pix);
            write_v(&mut yuv[..], i, j, avg_pix);
        }
    }
    yuv
}

pub struct YUV420Buf {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl av_data::frame::FrameBuffer for YUV420Buf {
    fn linesize(&self, idx: usize) -> Result<usize, av_data::frame::FrameError> {
        match idx {
            0 => Ok(self.width),
            1 | 2 => Ok(self.width / 2),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn count(&self) -> usize {
        3
    }

    fn as_slice_inner(&self, idx: usize) -> Result<&[u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u + (base_u / 4);
        match idx {
            0 => Ok(&self.data[0..self.width * self.height]),
            1 => Ok(&self.data[base_u..base_v]),
            2 => Ok(&self.data[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn as_mut_slice_inner(&mut self, idx: usize) -> Result<&mut [u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u + (base_u / 4);
        match idx {
            0 => Ok(&mut self.data[0..self.width * self.height]),
            1 => Ok(&mut self.data[base_u..base_v]),
            2 => Ok(&mut self.data[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }
}

#[cfg(feature = "all")]
impl YUVSource for YUV420Buf {
    fn width(&self) -> i32 {
        self.width as i32
    }

    fn height(&self) -> i32 {
        self.height as i32
    }

    fn y(&self) -> &[u8] {
        &self.data[0..self.width * self.height]
    }

    fn u(&self) -> &[u8] {
        let base = self.width * self.height;
        &self.data[base..base + base / 4]
    }

    fn v(&self) -> &[u8] {
        let base_u = self.width * self.height;
        let base_v = base_u + (base_u / 4);
        &self.data[base_v..]
    }

    fn y_stride(&self) -> i32 {
        self.width as _
    }

    fn u_stride(&self) -> i32 {
        self.width as i32 / 2
    }

    fn v_stride(&self) -> i32 {
        self.width as i32 / 2
    }
}

pub struct YUV444Buf {
    pub yuv: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

impl av_data::frame::FrameBuffer for YUV444Buf {
    fn linesize(&self, idx: usize) -> Result<usize, av_data::frame::FrameError> {
        match idx {
            0 => Ok(self.width),
            1..=2 => Ok(self.width),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn count(&self) -> usize {
        3
    }

    fn as_slice_inner(&self, idx: usize) -> Result<&[u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u * 2;
        match idx {
            0 => Ok(&self.yuv[0..base_u]),
            1 => Ok(&self.yuv[base_u..base_v]),
            2 => Ok(&self.yuv[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }

    fn as_mut_slice_inner(&mut self, idx: usize) -> Result<&mut [u8], av_data::frame::FrameError> {
        let base_u = self.width * self.height;
        let base_v = base_u * 2;
        match idx {
            0 => Ok(&mut self.yuv[0..self.width * self.height]),
            1 => Ok(&mut self.yuv[base_u..base_v]),
            2 => Ok(&mut self.yuv[base_v..]),
            _ => Err(av_data::frame::FrameError::InvalidIndex),
        }
    }
}
