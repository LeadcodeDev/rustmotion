pub fn composite_over(base: &mut [u8], overlay: &[u8]) {
    debug_assert_eq!(base.len(), overlay.len());
    let (base_px, _) = base.as_chunks_mut::<4>();
    let (overlay_px, _) = overlay.as_chunks::<4>();
    for (dst, src) in base_px.iter_mut().zip(overlay_px) {
        let inverse_source_alpha = 255u32 - src[3] as u32;
        for channel in 0..4 {
            let kept = (dst[channel] as u32 * inverse_source_alpha + 127) / 255;
            dst[channel] = (src[channel] as u32 + kept).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fully_transparent_overlay_leaves_the_base_untouched() {
        let mut base = vec![10, 20, 30, 255, 40, 50, 60, 255];
        let overlay = vec![0u8; 8];
        let before = base.clone();
        composite_over(&mut base, &overlay);
        assert_eq!(base, before);
    }

    #[test]
    fn an_opaque_overlay_replaces_the_base() {
        let mut base = vec![10, 20, 30, 255];
        let overlay = vec![200, 100, 50, 255];
        composite_over(&mut base, &overlay);
        assert_eq!(base, vec![200, 100, 50, 255]);
    }

    #[test]
    fn a_half_transparent_overlay_blends_towards_it() {
        let mut base = vec![0, 0, 0, 255];
        let overlay = vec![128, 128, 128, 128];
        composite_over(&mut base, &overlay);
        assert_eq!(base[3], 255);
        assert!(
            base[0] > 120 && base[0] < 140,
            "premultiplied src-over of a 50% grey onto black lands near 128, got {}",
            base[0]
        );
    }

    #[test]
    fn compositing_is_idempotent_for_a_transparent_overlay_whatever_the_base() {
        let mut base: Vec<u8> = (0..64).map(|i| (i * 3 % 256) as u8).collect();
        let before = base.clone();
        composite_over(&mut base, &[0u8; 64]);
        assert_eq!(base, before);
    }
}
