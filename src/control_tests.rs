#[cfg(test)]
mod tests {
    use crate::control::ControlCommand;
    
    #[test]
    fn test_control_command_creation() {
        // Test that all control commands can be created
        let _play = ControlCommand::Play;
        let _stop = ControlCommand::Stop;
        let _volume_up = ControlCommand::VolumeUp;
        let _volume_down = ControlCommand::VolumeDown;
        let _set_volume = ControlCommand::SetVolume(1.0);
        let _tune = ControlCommand::Tune("test".to_string());
        let _tune_next = ControlCommand::TuneNext;
        let _tune_prev = ControlCommand::TunePrev;
        let _select_up = ControlCommand::SelectUp;
        let _select_down = ControlCommand::SelectDown;
        let _toggle = ControlCommand::Toggle;
        let _toggle_pause = ControlCommand::TogglePause;
        let _toggle_help = ControlCommand::ToggleHelp;
        let _scroll_history_up = ControlCommand::ScrollHistoryUp;
        let _scroll_history_down = ControlCommand::ScrollHistoryDown;
        let _quit = ControlCommand::Quit;
    }
}