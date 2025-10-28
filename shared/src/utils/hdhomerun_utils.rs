const CHECKSUM_LOOKUP: [u8; 16] = [0xA, 0x5, 0xF, 0x6, 0x7, 0xC, 0x1, 0xB, 0x9, 0x2, 0x8, 0xD, 0x4, 0x3, 0xE, 0x0];

fn calculate_checksum(device_id_int: u32) -> u8 {
    let mut checksum: u8 = 0;
    checksum ^= CHECKSUM_LOOKUP[((device_id_int >> 28) & 0x0F) as usize];
    checksum ^= ((device_id_int >> 24) & 0x0F) as u8;
    checksum ^= CHECKSUM_LOOKUP[((device_id_int >> 20) & 0x0F) as usize];
    checksum ^= ((device_id_int >> 16) & 0x0F) as u8;
    checksum ^= CHECKSUM_LOOKUP[((device_id_int >> 12) & 0x0F) as usize];
    checksum ^= ((device_id_int >> 8) & 0x0F) as u8;
    checksum ^= CHECKSUM_LOOKUP[((device_id_int >> 4) & 0x0F) as usize];
    checksum
}

pub fn validate_hdhr_device_id(device_id: &str) -> bool {
    if device_id.len() != 8 || !device_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    if let Ok(device_id_int) = u32::from_str_radix(device_id, 16) {
        let checksum = calculate_checksum(device_id_int);
        return (device_id_int & 0x0F) as u8 == checksum;
    }
    false
}

pub fn generate_hdhr_device_id_from_base(base_id: &str) -> String {
    let base_sanitized = base_id.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>();
    let base_padded = if base_sanitized.is_empty() {
       return generate_hdhr_device_id();
    } else {
       format!("{:0<7}", &base_sanitized[..base_sanitized.len().min(7)])
    };

    if let Ok(device_id_int_base) = u32::from_str_radix(&base_padded, 16) {
        let checksum = calculate_checksum(device_id_int_base);
        let final_id = (device_id_int_base & 0xFFFFFFF0) | u32::from(checksum);
        format!("{:08X}", final_id)
    } else {
        generate_hdhr_device_id()
    }
}

pub fn generate_hdhr_device_id() -> String {
    let random_part: String = (0..4)
        .map(|_| format!("{:X}", fastrand::u8(0..16)))
        .collect();

    let base_id = format!("105{}0", random_part);
    generate_hdhr_device_id_from_base(&base_id)
}