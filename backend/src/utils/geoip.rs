use std::io;
use std::io::{BufRead};
use std::net::Ipv4Addr;
use std::path::Path;
use serde::{Serialize, Deserialize};
use crate::repository::bplustree::BPlusTree;

fn ipv4_to_u32(ip: &str) -> Option<u32> {
    ip.parse::<Ipv4Addr>().ok().map(u32::from)
}

#[derive(Serialize, Deserialize)]
pub struct GeoIp {
    tree: BPlusTree<u32, (u32, String)>,
}

impl GeoIp {

    pub fn load(path: &Path) -> io::Result<Self> {
        let tree = BPlusTree::load(path)?;
        Ok(Self { tree  })
    }

    pub fn new() -> Self {
        Self { tree: BPlusTree::new()  }
    }

    pub fn import_ipv4_from_csv(&mut self, mut reader: impl BufRead, db_path: &Path) -> std::io::Result<u64> {
        let mut buf = String::new();

        while reader.read_line(&mut buf)? > 0 {
            let line = buf.trim();
            if line.is_empty() || line.starts_with('#') { continue; }

            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() != 3 { continue; }

            if let (Some(start), Some(end)) = (ipv4_to_u32(parts[0]), ipv4_to_u32(parts[1])) {
                let cc = parts[2].trim().to_string();
                self.tree.insert(start, (end, cc));
            }
            buf.clear();
        }
        self.tree.store(db_path)
    }

    pub fn lookup(&self, ip_str: &str) -> Option<String> {
        let ip = ipv4_to_u32(ip_str)?;
        if let Some((_, (end, cc))) = self.tree.find_le(&ip) {
            if ip <= *end {
                return Some(cc.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    // https://raw.githubusercontent.com/sapics/ip-location-db/refs/heads/main/asn-country/asn-country-ipv4.csv

    use std::fs::File;
    use std::io::BufReader;
    use std::path::PathBuf;
    use crate::utils::geoip::GeoIp;

    #[test]
    pub fn test_csv() {
        let db_file =  PathBuf::from("/projects/m3u-test/asn-country-ipv4.db");
        let source = PathBuf::from("/projects/m3u-test/asn-country-ipv4.csv");
        let file = File::open(source).expect("Could not open csv file");
        let reader = BufReader::new(file);
        let mut geo_ip = GeoIp::new();
        let _ = geo_ip.import_ipv4_from_csv(reader, &db_file).expect("Could not import csv");

        let geo_ip = GeoIp::load(&db_file).expect("Failed to load geoip db");
        if let Some(cc) =  geo_ip.lookup("72.13.24.23") {
            assert_eq!(cc, "US");
        } else {
            assert!(false);
        }

    }
}