use skia_safe::{
    Canvas, CubicResampler, FilterMode, Image, Matrix, MipmapMode, Paint, Rect, SamplingOptions,
};

pub fn device_axis_scales(matrix: &Matrix) -> (f32, f32) {
    (
        matrix.scale_x().hypot(matrix.skew_y()),
        matrix.scale_y().hypot(matrix.skew_x()),
    )
}

pub fn photo_sampling(source: (f32, f32), on_screen: (f32, f32)) -> SamplingOptions {
    let minifying = on_screen.0 < source.0 || on_screen.1 < source.1;
    if minifying {
        SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear)
    } else {
        SamplingOptions::from(CubicResampler::mitchell())
    }
}

pub fn draw_photo(canvas: &Canvas, image: impl AsRef<Image>, dst: Rect, paint: &Paint) {
    let image = image.as_ref();
    let (scale_x, scale_y) = device_axis_scales(&canvas.local_to_device_as_3x3());
    let sampling = photo_sampling(
        (image.width() as f32, image.height() as f32),
        (dst.width() * scale_x, dst.height() * scale_y),
    );
    canvas.draw_image_rect_with_sampling_options(image, None, dst, sampling, paint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_safe::{images, surfaces, AlphaType, ColorType, Data, ImageInfo};

    const MIPMAPPED: SamplingOptions = SamplingOptions {
        max_aniso: 0,
        use_cubic: false,
        cubic: CubicResampler { b: 0.0, c: 0.0 },
        filter: FilterMode::Linear,
        mipmap: MipmapMode::Linear,
    };

    fn checkerboard(size: i32) -> Image {
        let mut pixels = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let info = ImageInfo::new((size, size), ColorType::RGBA8888, AlphaType::Premul, None);
        images::raster_from_data(&info, Data::new_copy(&pixels), (size * 4) as usize)
            .expect("raster image from checkerboard pixels")
    }

    struct Luma {
        mean: f64,
        min: u8,
        max: u8,
    }

    fn luma_inside(rgba: &[u8], side: u32, inset: u32) -> Luma {
        let mut values = Vec::new();
        for y in inset..side - inset {
            for x in inset..side - inset {
                let i = ((y * side + x) * 4) as usize;
                let luma = 0.299 * rgba[i] as f64
                    + 0.587 * rgba[i + 1] as f64
                    + 0.114 * rgba[i + 2] as f64;
                values.push(luma.round() as u8);
            }
        }
        Luma {
            mean: values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64,
            min: *values.iter().min().expect("non-empty region"),
            max: *values.iter().max().expect("non-empty region"),
        }
    }

    fn draw_checkerboard(side: u32, canvas_scale: f32, through_helper: bool) -> Vec<u8> {
        let info = ImageInfo::new(
            (side as i32, side as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surfaces::raster(&info, None, None).expect("raster surface");
        let source = checkerboard(640);
        let local_side = side as f32 / canvas_scale;
        let dst = Rect::from_xywh(0.0, 0.0, local_side, local_side);
        {
            let canvas = surface.canvas();
            canvas.scale((canvas_scale, canvas_scale));
            if through_helper {
                draw_photo(canvas, &source, dst, &Paint::default());
            } else {
                canvas.draw_image_rect(&source, None, dst, &Paint::default());
            }
        }
        let mut rgba = vec![0u8; (side * side * 4) as usize];
        assert!(surface.read_pixels(&info, &mut rgba, (side * 4) as usize, (0, 0)));
        rgba
    }

    #[test]
    fn minifying_either_axis_asks_for_mipmaps() {
        assert_eq!(photo_sampling((640.0, 640.0), (64.0, 64.0)), MIPMAPPED);
        assert_eq!(photo_sampling((640.0, 640.0), (64.0, 900.0)), MIPMAPPED);
        assert_eq!(photo_sampling((640.0, 640.0), (900.0, 64.0)), MIPMAPPED);
    }

    #[test]
    fn drawing_at_or_above_source_size_asks_for_cubic() {
        let at_source = photo_sampling((640.0, 640.0), (640.0, 640.0));
        let enlarged = photo_sampling((640.0, 640.0), (1280.0, 1280.0));
        assert!(at_source.use_cubic, "1:1 must not sample with mipmaps");
        assert!(enlarged.use_cubic, "an enlargement must not sample mipmaps");
    }

    #[test]
    fn axis_scales_ignore_rotation() {
        let (sx, sy) = device_axis_scales(&Matrix::scale((2.0, 3.0)));
        assert!((sx - 2.0).abs() < 1e-5 && (sy - 3.0).abs() < 1e-5);

        let mut rotated = Matrix::new_identity();
        rotated.set_rotate(90.0, None);
        let (rx, ry) = device_axis_scales(&rotated);
        assert!(
            (rx - 1.0).abs() < 1e-5 && (ry - 1.0).abs() < 1e-5,
            "a pure rotation is not a shrink, got ({rx}, {ry})"
        );
    }

    #[test]
    fn a_checkerboard_shrunk_to_a_tenth_averages_to_grey() {
        let before = luma_inside(&draw_checkerboard(64, 1.0, false), 64, 4);
        let after = luma_inside(&draw_checkerboard(64, 1.0, true), 64, 4);

        assert_eq!(
            (before.min, before.max),
            (255, 255),
            "the nearest-neighbour path is expected to read solid white at scale 1/10, \
             got min {} max {}",
            before.min,
            before.max
        );
        assert!(
            (after.mean - 127.0).abs() < 2.0 && after.min > 120 && after.max < 135,
            "a 1px checkerboard at scale 1/10 must average to grey, got mean {:.1} \
             min {} max {}",
            after.mean,
            after.min,
            after.max
        );
    }

    #[test]
    fn a_checkerboard_at_an_awkward_scale_averages_to_grey() {
        let before = luma_inside(&draw_checkerboard(120, 1.0, false), 120, 4);
        let after = luma_inside(&draw_checkerboard(120, 1.0, true), 120, 4);

        assert!(
            before.min == 0 && before.max == 255,
            "the nearest-neighbour path is expected to read black-and-white noise, \
             got min {} max {}",
            before.min,
            before.max
        );
        assert!(
            (after.mean - 127.0).abs() < 2.0 && after.min > 120 && after.max < 135,
            "a 1px checkerboard at scale 120/640 must average to grey, got mean {:.1} \
             min {} max {}",
            after.mean,
            after.min,
            after.max
        );
    }

    #[test]
    fn a_camera_zoom_out_is_read_as_a_shrink() {
        let full_size_rect = luma_inside(&draw_checkerboard(64, 0.1, true), 64, 4);
        assert!(
            (full_size_rect.mean - 127.0).abs() < 2.0 && full_size_rect.max < 135,
            "a 640px rect on a canvas scaled to 0.1 is drawn at 64px and must sample as a \
             shrink, got mean {:.1} max {}",
            full_size_rect.mean,
            full_size_rect.max
        );
    }
}
