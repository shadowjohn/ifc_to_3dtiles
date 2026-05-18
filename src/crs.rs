use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy)]
pub struct LonLat {
    pub lon_deg: f64,
    pub lat_deg: f64,
}

pub fn project_to_wgs84(source_epsg: u32, x: f64, y: f64) -> Result<LonLat> {
    if source_epsg != 3826 {
        bail!("目前只內建 EPSG:3826，收到 EPSG:{source_epsg}");
    }

    let from = proj4rs::Proj::from_proj_string(
        "+proj=tmerc +lat_0=0 +lon_0=121 +k=0.9999 +x_0=250000 +y_0=0 +ellps=GRS80 +units=m +no_defs +type=crs",
    )?;
    let to = proj4rs::Proj::from_proj_string("+proj=longlat +ellps=WGS84 +datum=WGS84 +no_defs")?;
    let mut point = (x, y, 0.0);
    proj4rs::transform::transform(&from, &to, &mut point)?;
    Ok(LonLat {
        lon_deg: point.0.to_degrees(),
        lat_deg: point.1.to_degrees(),
    })
}

pub fn ecef_from_lon_lat_height(lon_deg: f64, lat_deg: f64, height: f64) -> [f64; 3] {
    let a = 6378137.0;
    let f = 1.0 / 298.257223563;
    let e2 = f * (2.0 - f);
    let lon = lon_deg.to_radians();
    let lat = lat_deg.to_radians();
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    [
        (n + height) * cos_lat * lon.cos(),
        (n + height) * cos_lat * lon.sin(),
        (n * (1.0 - e2) + height) * sin_lat,
    ]
}

pub fn enu_to_ecef_transform(lon_deg: f64, lat_deg: f64, height: f64) -> [f64; 16] {
    let lon = lon_deg.to_radians();
    let lat = lat_deg.to_radians();
    let sin_lon = lon.sin();
    let cos_lon = lon.cos();
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let origin = ecef_from_lon_lat_height(lon_deg, lat_deg, height);

    let east = [-sin_lon, cos_lon, 0.0];
    let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
    let up = [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat];

    [
        east[0], east[1], east[2], 0.0, north[0], north[1], north[2], 0.0, up[0], up[1], up[2],
        0.0, origin[0], origin[1], origin[2], 1.0,
    ]
}
