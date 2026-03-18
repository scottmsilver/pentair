# Test Fixtures

Binary protocol captures from a live ScreenLogic adapter.

## System at capture time
- **Date**: 2026-03-17 ~20:30 UTC
- **Adapter**: Pentair: 21-A9-F0 at 192.168.1.89:80
- **Firmware**: POOL: 5.2 Build 738.0 Rel
- **Controller**: IntelliTouch (type=1, hw_type=0)
- **Equipment flags**: 0x18 (IntelliBrite + IntelliFlow_0)
- **Circuits**: 7 (Spa, Lights, Water Feature, Jets, Floor Cleaner, Pool, Yard Light)
- **Colors**: 8 IntelliBrite presets
- **Temp unit**: Fahrenheit
- **Pool/Spa range**: 40-104°F
- **IntelliChem**: Not installed (chem data all zeros, sentinel=42)
- **IntelliChlor**: Check SCG config
- **Pumps**: Pump 0 has data (132 bytes), pumps 1-7 empty

## Files

Each `.bin` file contains the raw bytes as they would appear on the TCP wire (including the 8-byte message header for response files). Discovery request/response are UDP payloads.

| File | Size | Description |
|------|------|-------------|
| discovery_request.bin | 8 | UDP broadcast payload |
| discovery_response.bin | 40 | UDP response with IP, port, name |
| connect_string.bin | 21 | Raw TCP init: CONNECTSERVERHOST\r\n\r\n |
| login_request.bin | 52 | Login message (action 27) |
| login_response.bin | 8 | Login accepted (action 28), header only |
| version_response.bin | 64 | Firmware version string |
| controller_config_response.bin | 468 | Full config: circuits, colors, flags |
| status_response.bin | 192 | Equipment state: temps, circuits, chem |
| chem_data_response.bin | 56 | IntelliChem data (all zeros, no IC installed) |
| scg_config_response.bin | 36 | Chlorinator config |
| pump_status_0_response.bin | 132 | Pump 0 (IntelliFlow): type, watts, rpm, gpm, circuits |
| pump_status_[1-7]_response.bin | 8 each | Pumps 1-7: header only (no pump) |
| schedule_recurring_response.bin | 940 | Recurring schedules |
| schedule_runonce_response.bin | 12 | Run-once schedules (empty) |
| weather_response.bin | 96 | Weather forecast data |
| system_time_response.bin | 28 | Controller system time |
