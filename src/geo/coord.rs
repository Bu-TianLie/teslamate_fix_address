//! Coordinate conversion: WGS84 ↔ GCJ02 ↔ BD09
//! All output rounded to 6 decimal places (numeric(8,6) precision).

use std::f64::consts::PI;

const X_PI: f64 = PI * 3000.0 / 180.0;
const A: f64 = 6378245.0;
const EE: f64 = 0.00669342162296594323;

/// Round to 6 decimal places (numeric(8,6) precision).
fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

/// Is the point inside mainland China (approximate)?
pub fn is_in_china(lat: f64, lng: f64) -> bool {
    lng >= 72.004 && lng <= 137.8347 && lat >= 0.8293 && lat <= 55.8271
}

fn transform_lat(x: f64, y: f64) -> f64 {
    let mut ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y
        + 0.1 * x * y
        + 0.2 * (x.abs()).sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    ret += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * 2.0 / 3.0;
    ret += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y / 30.0 * PI).sin()) * 2.0 / 3.0;
    ret
}

fn transform_lng(x: f64, y: f64) -> f64 {
    let mut ret = 300.0 + x + 2.0 * y + 0.1 * x * x
        + 0.1 * x * y
        + 0.1 * (x.abs()).sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    ret += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * 2.0 / 3.0;
    ret += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * 2.0 / 3.0;
    ret
}

/// WGS84 → GCJ02 (for Tencent / Amap)
pub fn wgs84_to_gcj02(lat: f64, lng: f64) -> (f64, f64) {
    if !is_in_china(lat, lng) {
        return (lat, lng);
    }
    let d_lat = transform_lat(lng - 105.0, lat - 35.0);
    let d_lng = transform_lng(lng - 105.0, lat - 35.0);
    let rad_lat = lat / 180.0 * PI;
    let magic = rad_lat.sin();
    let magic_sq = 1.0 - EE * magic * magic;
    let sqrt_magic = magic_sq.sqrt();
    let d_lat = (d_lat * 180.0) / ((A * (1.0 - EE)) / (magic_sq * sqrt_magic) * PI);
    let d_lng = (d_lng * 180.0) / (A / sqrt_magic * rad_lat.cos() * PI);
    (round6(lat + d_lat), round6(lng + d_lng))
}

pub fn gcj02_to_wgs84_exact(lat: f64, lng: f64) -> (f64, f64) {
    let mut wgs_lat = lat;
    let mut wgs_lng = lng;

    for _ in 0..10 {
        let (tmp_lat, tmp_lng) = wgs84_to_gcj02(wgs_lat, wgs_lng);
        let d_lat = tmp_lat - lat;
        let d_lng = tmp_lng - lng;

        wgs_lat -= d_lat;
        wgs_lng -= d_lng;

        if d_lat.abs() < 1e-7 && d_lng.abs() < 1e-7 {
            break;
        }
    }

    (round6(wgs_lat), round6(wgs_lng))
}

/// GCJ02 → WGS84 (approximate inverse)
pub fn gcj02_to_wgs84(lat: f64, lng: f64) -> (f64, f64) {
    if !is_in_china(lat, lng) {
        return (lat, lng);
    }
    let d_lat = transform_lat(lng - 105.0, lat - 35.0);
    let d_lng = transform_lng(lng - 105.0, lat - 35.0);
    let rad_lat = lat / 180.0 * PI;
    let magic = rad_lat.sin();
    let magic_sq = 1.0 - EE * magic * magic;
    let sqrt_magic = magic_sq.sqrt();
    let d_lat = (d_lat * 180.0) / ((A * (1.0 - EE)) / (magic_sq * sqrt_magic) * PI);
    let d_lng = (d_lng * 180.0) / (A / sqrt_magic * rad_lat.cos() * PI);
    let mg_lat = lat + d_lat;
    let mg_lng = lng + d_lng;
    (round6(2.0 * lat - mg_lat), round6(2.0 * lng - mg_lng))
}

/// GCJ02 → BD09 (for Baidu)
pub fn gcj02_to_bd09(lat: f64, lng: f64) -> (f64, f64) {
    let z = (lng * lng + lat * lat).sqrt() + 0.00002 * (lat * X_PI).sin();
    let theta = lat.atan2(lng) + 0.000003 * (lng * X_PI).cos();
    let bd_lng = z * theta.cos() + 0.0065;
    let bd_lat = z * theta.sin() + 0.006;
    (round6(bd_lat), round6(bd_lng))
}

/// BD09 → GCJ02
pub fn bd09_to_gcj02(lat: f64, lng: f64) -> (f64, f64) {
    let x = lng - 0.0065;
    let y = lat - 0.006;
    let z = (x * x + y * y).sqrt() - 0.00002 * (y * X_PI).sin();
    let theta = y.atan2(x) - 0.000003 * (x * X_PI).cos();
    let gcj_lng = z * theta.cos();
    let gcj_lat = z * theta.sin();
    (round6(gcj_lat), round6(gcj_lng))
}

/// WGS84 → BD09 (convenience)
pub fn wgs84_to_bd09(lat: f64, lng: f64) -> (f64, f64) {
    let (glat, glng) = wgs84_to_gcj02(lat, lng);
    gcj02_to_bd09(glat, glng)
}

#[cfg(test)]
mod tests {
   

    use super::*;

    #[test]
    fn test_wgs84_gcj02_roundtrip() {
        let (lat, lng) = wgs84_to_gcj02(30.339256, 120.112903);
        println!("Converted: lat={}, lng={}", lat, lng);
    
        let (back_lat, back_lng) = gcj02_to_wgs84_exact(lat, lng);
        println!("Back: lat={}, lng={}", back_lat, back_lng);
        assert!((back_lat - 30.339256).abs() < 0.000001);
        assert!((back_lng - 120.112903).abs() < 0.000001);
    }

    #[test]
    fn test_outside_china_unchanged() {
        let (lat, lng) = wgs84_to_gcj02(51.5074, -0.1278);
        assert!((lat - 51.5074).abs() < 1e-10);
        assert!((lng - (-0.1278)).abs() < 1e-10);
    }

    #[test]
    fn test_round6_precision() {
        let (lat, lng) = wgs84_to_gcj02(39.904201, 116.407401);
        // Must have at most 6 decimal places
        let lat_str = format!("{:.6}", lat);
        let lng_str = format!("{:.6}", lng);
        assert_eq!(lat_str.parse::<f64>().unwrap(), lat);
        assert_eq!(lng_str.parse::<f64>().unwrap(), lng);
    }
}
