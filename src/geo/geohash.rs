//! GeoHash encoding/decoding for coordinate clustering.
//! Precision 7 ≈ 153m × 153m — sufficient for parking-level geocoding.

const BASE32: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// Encode (lat, lng) to a GeoHash string with given precision (1..=12).
pub fn encode(lat: f64, lng: f64, precision: usize) -> String {
    assert!((1..=12).contains(&precision), "precision must be 1..12");

    let mut min_lat = -90.0_f64;
    let mut max_lat = 90.0_f64;
    let mut min_lng = -180.0_f64;
    let mut max_lng = 180.0_f64;

    let mut hash = String::with_capacity(precision);
    let mut bit = 0_u8;
    let mut ch = 0_u8;
    let mut is_lng = true;

    while hash.len() < precision {
        if is_lng {
            let mid = (min_lng + max_lng) / 2.0;
            if lng >= mid {
                ch |= 1 << (4 - bit);
                min_lng = mid;
            } else {
                max_lng = mid;
            }
        } else {
            let mid = (min_lat + max_lat) / 2.0;
            if lat >= mid {
                ch |= 1 << (4 - bit);
                min_lat = mid;
            } else {
                max_lat = mid;
            }
        }

        is_lng = !is_lng;

        if bit == 4 {
            hash.push(BASE32[ch as usize] as char);
            ch = 0;
            bit = 0;
        } else {
            bit += 1;
        }
    }

    hash
}

/// Decode a GeoHash string back to (lat, lng, lat_error, lng_error).
pub fn decode(hash: &str) -> (f64, f64, f64, f64) {
    let mut min_lat = -90.0_f64;
    let mut max_lat = 90.0_f64;
    let mut min_lng = -180.0_f64;
    let mut max_lng = 180.0_f64;

    let mut is_lng = true;

    for ch in hash.bytes() {
        let val = BASE32
            .iter()
            .position(|&b| b == ch)
            .unwrap_or_else(|| panic!("invalid geohash char: {}", ch as char));

        for bit in (0..5).rev() {
            if is_lng {
                let mid = (min_lng + max_lng) / 2.0;
                if val & (1 << bit) != 0 {
                    min_lng = mid;
                } else {
                    max_lng = mid;
                }
            } else {
                let mid = (min_lat + max_lat) / 2.0;
                if val & (1 << bit) != 0 {
                    min_lat = mid;
                } else {
                    max_lat = mid;
                }
            }
            is_lng = !is_lng;
        }
    }

    let lat = (min_lat + max_lat) / 2.0;
    let lng = (min_lng + max_lng) / 2.0;
    let lat_err = (max_lat - min_lat) / 2.0;
    let lng_err = (max_lng - min_lng) / 2.0;

    (lat, lng, lat_err, lng_err)
}

/// Compute the centroid (average) of a set of (lat, lng) points.
pub fn centroid(points: &[(f64, f64)]) -> (f64, f64) {
    assert!(!points.is_empty());
    let n = points.len() as f64;
    let mut lat_sum = 0.0;
    let mut lng_sum = 0.0;
    for &(lat, lng) in points {
        lat_sum += lat;
        lng_sum += lng;
    }
    (lat_sum / n, lng_sum / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let lat = 39.9042;
        let lng = 116.4074;
        let hash = encode(lat, lng, 9);
        let (dlat, dlng, _, _) = decode(&hash);
        assert!((dlat - lat).abs() < 0.001);
        assert!((dlng - lng).abs() < 0.001);
    }

    #[test]
    fn test_cluster_key() {
        // 两个距离很近的点，precision=7 应该在同一 cluster
        let h1 = encode(39.9042, 116.4074, 7);
        let h2 = encode(39.9043, 116.4075, 7);
        assert_eq!(h1, h2);

        // precision=12 不一定相同
        let h3 = encode(39.9042, 116.4074, 12);
        let h4 = encode(39.9043, 116.4075, 12);
        // 可能相同也可能不同，但至少前 7 位相同
        assert_eq!(&h3[..7], &h4[..7]);
    }

    #[test]
    fn test_centroid() {
        let pts = vec![(39.90, 116.40), (39.91, 116.41)];
        let (clat, clng) = centroid(&pts);
        assert!((clat - 39.905).abs() < 1e-10);
        assert!((clng - 116.405).abs() < 1e-10);
    }
}
