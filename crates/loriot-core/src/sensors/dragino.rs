//! Dragino LHT65S-PIR sensor decoder
//!
//! This module decodes the binary payload from Dragino LHT65S-PIR sensors
//! which include temperature, humidity, battery, PIR motion detection, and lux.

use super::SensorReading;

/// Decoded data from LHT65S-PIR sensor
/// Returns: (battery_voltage, battery_status, temperature, humidity, activity, movement_count, lux, info_byte)
pub type Lht65sPirData = (f32, String, f32, f32, bool, u32, Option<u16>, u8);

/// Decode LHT65S-PIR sensor data from Dragino
///
/// # Arguments
/// * `data` - Hex-encoded payload (minimum 22 characters for 11 bytes)
///
/// # Returns
/// * `Some(Lht65sPirData)` - Decoded sensor values
/// * `None` - If the data format is invalid
///
/// # Payload Format (11+ bytes)
/// - Bytes 0-1: Battery info with status bits
/// - Bytes 2-3: Temperature (signed i16 / 100) in Celsius
/// - Bytes 4-5: Humidity (u16 / 10) in percentage
/// - Bytes 6-7: Battery voltage (u16 / 1000) in volts
/// - Byte 8 (LSB): PIR activity flag
/// - Bytes 8-9: Movement count (u16 with PIR bit masked)
/// - Bytes 10-11: Lux data (optional, u16) for E5 variant
pub fn decode_lht65s_pir(data: &str) -> Option<Lht65sPirData> {
    // Minimum 22 hex characters (11 bytes)
    if data.len() < 22 {
        return None;
    }

    let bytes = hex::decode(data).ok()?;
    if bytes.len() < 11 {
        return None;
    }

    // Temperature (bytes 2-3): signed 16-bit, /100
    let temp_raw = ((bytes[2] as u16) << 8 | bytes[3] as u16) as i16;
    let temperature = temp_raw as f32 / 100.0;

    // Humidity (bytes 4-5): unsigned 16-bit, /10
    let humidity_raw = (bytes[4] as u16) << 8 | bytes[5] as u16;
    let humidity = humidity_raw as f32 / 10.0;

    // Battery voltage (bytes 6-7): unsigned 16-bit, /1000
    let battery_raw = (bytes[6] as u16) << 8 | bytes[7] as u16;
    let battery_voltage = battery_raw as f32 / 1000.0;

    // Battery status based on voltage
    let battery_status = if battery_voltage > 3.0 {
        "Good".to_string()
    } else if battery_voltage > 2.8 {
        "Low".to_string()
    } else {
        "Critical".to_string()
    };

    // PIR activity (byte 8, bit 0)
    let activity = (bytes[8] & 0x01) != 0;

    // Movement count (bytes 8-9): 16-bit with PIR bit masked out
    let movement_raw = ((bytes[8] & 0xFE) as u16) << 8 | bytes[9] as u16;
    let movement_count = (movement_raw >> 1) as u32;

    // Lux data (bytes 10-11): optional 16-bit (requires 12+ bytes)
    let lux = if bytes.len() >= 12 {
        Some((bytes[10] as u16) << 8 | bytes[11] as u16)
    } else {
        None
    };

    // Additional info byte (could be useful for future extensions)
    let info_byte = bytes.first().copied().unwrap_or(0);

    Some((
        battery_voltage,
        battery_status,
        temperature,
        humidity,
        activity,
        movement_count,
        lux,
        info_byte,
    ))
}

/// Create a SensorReading from parsed LHT65S-PIR data
pub fn create_sensor_reading(
    cmd: String,
    eui: String,
    data: String,
    ts: u64,
    name: Option<String>,
    parsed_data: Lht65sPirData,
) -> SensorReading {
    let (battery_voltage, battery_status, temperature, humidity, activity, movement_count, lux, _) =
        parsed_data;

    SensorReading {
        cmd,
        eui,
        data,
        ts,
        name,
        parsed_temperature: Some(temperature),
        parsed_humidity: Some(humidity),
        parsed_battery_voltage: Some(battery_voltage),
        parsed_battery_status: Some(battery_status),
        parsed_activity: Some(activity),
        parsed_movement_count: Some(movement_count),
        parsed_lux: lux,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_valid_payload() {
        // Example payload from a real LHT65S-PIR sensor (11 bytes minimum)
        // Format: 2 bat + 2 temp + 2 hum + 2 batvolt + 2 pir/count + 1 extra
        let payload = "cb0b0b9002430d9c000200";
        let result = decode_lht65s_pir(payload);
        assert!(result.is_some(), "Failed to decode valid 11-byte payload");

        let (voltage, status, temp, humidity, _activity, count, _lux, _) = result.unwrap();
        assert!(voltage > 0.0);
        assert!(!status.is_empty());
        assert!((-40.0..85.0).contains(&temp));
        assert!((0.0..=100.0).contains(&humidity));
        assert!(count < 65535);
    }

    #[test]
    fn test_decode_invalid_length() {
        assert!(decode_lht65s_pir("short").is_none());
        assert!(decode_lht65s_pir("invalidhex!!").is_none());
    }
}
