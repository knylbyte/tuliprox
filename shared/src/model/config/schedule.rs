
pub fn validate_cron(expr: &str) -> Result<(), String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 7 {
        return Err("Cron expression must have 7 fields".into());
    }

    // helper: check valid range
    fn check_field(val: &str, min: u32, max: u32) -> bool {
        val == "*" ||
            val.split(',').all(|chunk| {
                if let Ok(num) = chunk.parse::<u32>() {
                    num >= min && num <= max
                } else if let Some((from, to)) = chunk.split_once('-') {
                    if let (Ok(f), Ok(t)) = (from.parse::<u32>(), to.parse::<u32>()) {
                        f <= t && f >= min && t <= max
                    } else { false }
                } else {
                    false
                }
            })
    }

    let ranges = [
        (0, 59), // seconds
        (0, 59), // minutes
        (0, 23), // hours
        (1, 31), // day of month
        (1, 12), // month
        (0, 6),  // day of week
        (1970, 2099), // year (example range)
    ];

    for (field, (min, max)) in parts.iter().zip(ranges.iter()) {
        if !check_field(field, *min, *max) {
            return Err(format!("Invalid field '{}', expected {}-{}", field, min, max));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleConfigDto {
    #[serde(default)]
    pub schedule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
}