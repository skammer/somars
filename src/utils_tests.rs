#[cfg(test)]
mod tests {
    use crate::utils::format_duration;
    
    #[test]
    fn test_format_duration() {
        // Test zero duration
        let duration = std::time::Duration::from_secs(0);
        assert_eq!(format_duration(duration), "00:00:00");
        
        // Test seconds only
        let duration = std::time::Duration::from_secs(30);
        assert_eq!(format_duration(duration), "00:00:30");
        
        // Test minutes and seconds
        let duration = std::time::Duration::from_secs(90); // 1 minute 30 seconds
        assert_eq!(format_duration(duration), "00:01:30");
        
        // Test hours, minutes and seconds
        let duration = std::time::Duration::from_secs(3661); // 1 hour 1 minute 1 second
        assert_eq!(format_duration(duration), "01:01:01");
    }
}