/// Convert RGBA pixels to YUV420 (I420) for H.264 encoding.
/// Uses integer BT.601 arithmetic and rayon parallelization for performance.
pub fn rgba_to_yuv420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    use rayon::prelude::*;

    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_w = w / 2;
    let uv_h = h / 2;
    let uv_size = uv_w * uv_h;
    let mut yuv = vec![0u8; y_size + 2 * uv_size];

    let (y_plane, uv_planes) = yuv.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    // Compute Y plane in parallel (one row per task)
    y_plane
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(row, y_row)| {
            let row_offset = row * w * 4;
            for col in 0..w {
                let idx = row_offset + col * 4;
                let r = rgba[idx] as i32;
                let g = rgba[idx + 1] as i32;
                let b = rgba[idx + 2] as i32;
                y_row[col] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
            }
        });

    // Compute U and V planes in parallel (one row-pair per task)
    let uv_combined: Vec<(u8, u8)> = (0..uv_h)
        .into_par_iter()
        .flat_map(|uv_row| {
            let row = uv_row * 2;
            (0..uv_w)
                .map(move |uv_col| {
                    let col = uv_col * 2;
                    let mut r_sum = 0i32;
                    let mut g_sum = 0i32;
                    let mut b_sum = 0i32;
                    for dr in 0..2 {
                        for dc in 0..2 {
                            let idx = ((row + dr) * w + (col + dc)) * 4;
                            r_sum += rgba[idx] as i32;
                            g_sum += rgba[idx + 1] as i32;
                            b_sum += rgba[idx + 2] as i32;
                        }
                    }
                    let r = r_sum >> 2;
                    let g = g_sum >> 2;
                    let b = b_sum >> 2;
                    let u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                    let v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                    (u, v)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    for (i, (u, v)) in uv_combined.into_iter().enumerate() {
        u_plane[i] = u;
        v_plane[i] = v;
    }

    yuv
}
