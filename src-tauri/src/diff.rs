use std::time::{Duration, Instant};

use image::{imageops, DynamicImage};

pub fn compute_dhash(image: &DynamicImage) -> u64 {
    let start = Instant::now();

    let resized = image.resize_exact(9, 8, imageops::FilterType::Lanczos3);
    let gray = resized.to_luma8();

    let mut hash = 0_u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = gray.get_pixel(x, y)[0] as u64;
            let right = gray.get_pixel(x + 1, y)[0] as u64;
            hash = (hash << 1) | u64::from(left < right);
        }
    }

    let elapsed = start.elapsed();
    if elapsed > Duration::from_millis(50) {
        eprintln!("dhash exceeded target duration: {:?}", elapsed);
    }

    hash
}

pub fn hamming_distance(hash1: u64, hash2: u64) -> u32 {
    (hash1 ^ hash2).count_ones()
}

pub fn has_significant_change(hash1: u64, hash2: u64, threshold: u32) -> bool {
    hamming_distance(hash1, hash2) >= threshold
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::{DynamicImage, ImageBuffer, Luma};

    use super::{compute_dhash, hamming_distance, has_significant_change};

    fn gradient_image(reverse: bool) -> DynamicImage {
        let image = ImageBuffer::from_fn(128, 128, |x, _| {
            let value = if reverse {
                255_u8.saturating_sub((x * 2) as u8)
            } else {
                (x * 2) as u8
            };
            Luma([value])
        });

        DynamicImage::ImageLuma8(image)
    }

    #[test]
    fn identical_images_have_zero_hamming_distance() {
        let image = gradient_image(false);
        let hash1 = compute_dhash(&image);
        let hash2 = compute_dhash(&image);

        assert_eq!(hamming_distance(hash1, hash2), 0);
    }

    #[test]
    fn inverse_gradients_produce_large_distance() {
        let hash1 = compute_dhash(&gradient_image(false));
        let hash2 = compute_dhash(&gradient_image(true));

        assert!(
            hamming_distance(hash1, hash2) >= 32,
            "expected inverse gradients to differ significantly"
        );
    }

    #[test]
    fn threshold_10_detects_meaningful_change() {
        let hash1 = compute_dhash(&gradient_image(false));
        let hash2 = compute_dhash(&gradient_image(true));

        assert!(has_significant_change(hash1, hash2, 10));
        assert!(!has_significant_change(hash1, hash1, 10));
    }

    #[test]
    fn dhash_computation_stays_within_target_for_small_image() {
        let image = gradient_image(false);
        let _ = compute_dhash(&image);
        let start = Instant::now();
        let iterations = 10;

        for _ in 0..iterations {
            let _ = compute_dhash(&image);
        }

        let average_elapsed = start.elapsed() / iterations;

        assert!(
            average_elapsed < Duration::from_millis(50),
            "average dhash should finish within 50ms for a small test image"
        );
    }
}
