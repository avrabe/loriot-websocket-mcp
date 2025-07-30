use crate::sensors::SensorReading;

/// Decode LHT65S-PIR sensor data from Dragino
/// Format: 11 bytes binary payload with temperature, humidity, battery, PIR, and lux data
pub fn decode_lht65s_pir(data: &str) -> Option<(f32, String, f32, f32, bool, u32, Option<u16>, u8)> {
    if data.len() != 22 {
        return None;
    }
    
    let bytes = hex::decode(data).ok()?;
    if bytes.len() != 11 {
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
    
    // Lux data (bytes 10-11): optional 16-bit
    let lux = if bytes.len() > 10 {
        Some((bytes[10] as u16) << 8 | bytes[11] as u16)
    } else {
        None
    };
    
    // Additional info byte (could be useful for future extensions)
    let info_byte = bytes.get(0).copied().unwrap_or(0);
    
    Some((
        temperature,
        battery_status,
        humidity,
        battery_voltage,
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
    parsed_data: (f32, String, f32, f32, bool, u32, Option<u16>, u8),
) -> SensorReading {
    let (temperature, battery_status, humidity, battery_voltage, activity, movement_count, lux, _) = parsed_data;
    
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