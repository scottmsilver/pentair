# Pentair ScreenLogic IP Protocol — Byte-Level Reference

> Field status legend:
> - `[V]` Verified against live hardware (192.168.1.89, fw 5.2 Build 738.0)
> - `[X]` Cross-checked (node-screenlogic + screenlogicpy agree)
> - `[D]` Disputed (implementations disagree — see notes)
> - `[?]` Unknown / undocumented (bytes exist but meaning unclear)
> - `[N]` Not tested (no live data, inferred from reference implementations only)
> - `[~]` Partially understood (some bits decoded, others not)

All values little-endian unless marked BE. Offsets are relative to payload start (after 8-byte message header).

---

## 1. MESSAGE FRAMING

Every TCP message:

```
 Offset  Size  Type     Field            Status
 ──────  ────  ───────  ───────────────  ──────
 0       2     u16 LE   header_id        [V] Always 0 for client-originated
 2       2     u16 LE   action           [V] Message type code
 4       4     u32 LE   data_length      [V] Byte count of payload
 8..     N     bytes    payload          [V]
```

The initial connection uses raw TCP, not this framing:
```
 "CONNECTSERVERHOST\r\n\r\n"   (21 bytes, raw ASCII, no header)  [V]
```

---

## 2. UDP DISCOVERY (port 1444)

### Request → broadcast to 255.255.255.255:1444

```
 Offset  Size  Value    Status
 ──────  ────  ───────  ──────
 0       1     0x01     [V]
 1       7     0x00     [V]
```

### Response (observed: 40 bytes)

```
 Offset  Size  Type     Field            Status  Live Value
 ──────  ────  ───────  ───────────────  ──────  ──────────
 0       4     i32 LE   check_digit      [V]     2
 4       1     u8       ip_octet_1       [V]     192
 5       1     u8       ip_octet_2       [V]     168
 6       1     u8       ip_octet_3       [V]     1
 7       1     u8       ip_octet_4       [V]     89
 8       2     u16 LE   port             [V]     80
 10      1     u8       gateway_type     [V]     2
 11      1     u8       gateway_subtype  [V]     12
 12      28    ascii    adapter_name     [V]     "Pentair: 21-A9-F0"
```

**Gaps:**
- `[?]` What do gateway_type values mean? Only seen 2.
- `[?]` What do gateway_subtype values mean? Only seen 12.
- `[?]` Is the name field fixed-length 28 bytes or variable? (Null-terminated within 28 bytes observed)
- `[?]` Can multiple adapters respond? (Only 1 on this network)

---

## 3. CONTROL MESSAGES

### 3.1 Challenge Request (action 14 → response 15)

```
 REQUEST: action=14, payload=empty (0 bytes)         [V]

 RESPONSE action=15:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     u32 LE    string_length    [N]
 4       N     ascii     challenge_str    [N] MAC address of adapter
 4+N     pad   zeros     4-byte align     [N]
```

**Gaps:**
- `[?]` Is the challenge string always a MAC address?
- `[?]` What format? (e.g., "00-11-22-33-44-55" or "001122334455"?)
- `[?]` Is challenge required for local passwordless connections? (We skip it and login succeeds)

### 3.2 Login (action 27 → response 28, failure 13)

```
 REQUEST action=27:
 Offset  Size  Type      Field            Status  Value Used
 ──────  ────  ────────  ───────────────  ──────  ──────────
 0       4     i32 LE    schema           [V]     348
 4       4     i32 LE    connection_type  [V]     0
 8       4+N   SLString  client_version   [V]     "Android"
 8+N'    4+16  SLArray   password         [V]     16 zero bytes
 8+N'+20 4     i32 LE    process_id       [V]     2

 RESPONSE action=28 (success):
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       16    bytes     unknown          [?] All zeros observed
```

**Gaps:**
- `[?]` What does schema=348 mean? Is there a newer schema version?
- `[?]` What values can connection_type be? 0=local, 1=remote?
- `[?]` The 16-byte login response payload — what is it? Always zeros for local?
- `[D]` Password encoding: node-screenlogic sends SLArray(16 null bytes), screenlogicpy sends SLString("0000000000000000"). Both work locally.
- `[?]` process_id=2 — what do other values do? screenlogicpy uses 2, node-screenlogic uses 2.

### 3.3 Version (action 8120 → response 8121)

```
 REQUEST: action=8120, payload=empty                  [V]

 RESPONSE action=8121:
 Offset  Size  Type      Field            Status  Live Value
 ──────  ────  ────────  ───────────────  ──────  ──────────
 0       4+N   SLString  version_string   [V]     "POOL: 5.2 Build 738.0 Rel"
```

**Gaps:**
- `[?]` What other version string formats exist? (IntelliCenter, EasyTouch?)
- `[?]` Is there a structured way to parse the version number from the string?

### 3.4 Ping (action 16 → response 17)

```
 REQUEST: action=16, payload=empty                    [N]
 RESPONSE: action=17, payload=empty                   [N]
```

### 3.5 AddClient (action 12522 → response 12523)

```
 REQUEST action=12522:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [N] Use 0
 4       4     i32 LE    client_id        [N] Random 32767-65535

 RESPONSE action=12523:
 (ack, no meaningful payload)                         [N]
```

### 3.6 RemoveClient (action 12524 → response 12525)

```
 REQUEST action=12524:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [N]
 4       4     i32 LE    client_id        [N]

 RESPONSE action=12525:
 (ack)                                                [N]
```

---

## 4. CONTROLLER CONFIG (action 12532 → response 12533)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    param1           [V] Always 0
 4       4     i32 LE    param2           [V] Always 0
```

**Gaps:**
- `[?]` screenlogicpy notes that param2=1 returns a DIFFERENT compact config format (7 bytes per circuit, no names). Undocumented alternative.

### Response (460 bytes observed)

```
 Offset  Size  Type      Field              Status  Live Value
 ──────  ────  ────────  ──────────────── ──────  ──────────
 0       4     i32 LE    controller_id      [V]     100 (subtract 99 → 1)
 4       1     u8        pool_min_setpoint  [V]     40
 5       1     u8        pool_max_setpoint  [V]     104
 6       1     u8        spa_min_setpoint   [V]     40
 7       1     u8        spa_max_setpoint   [V]     104
 8       1     u8        is_celsius         [V]     0 (Fahrenheit)
 9       1     u8        controller_type    [V]     1 (IntelliTouch)
 10      1     u8        hw_type            [V]     0
 11      1     u8        controller_data    [V]     (observed)
 12      4     i32 LE    equipment_flags    [V]     24 (0x18 = IntelliBrite | IntelliFlow_0)
 16      4+N   SLString  generic_circ_name  [V]     "Water Features"
 ...     4     i32 LE    circuit_count      [V]     7

 Per circuit (×7):
 ┌──────  ────  ────────  ──────────────── ──────
 │ 0     4     i32 LE    circuit_id         [V] Wire value (subtract 499 for logical)
 │ 4     4+N   SLString  name               [V] e.g., "Spa", "Pool", "Lights"
 │ 4+N'  1     u8        name_index         [X]
 │ +1    1     u8        function           [V] 0=Generic, 1=Spa, 2=Pool, 16=IntelliBrite
 │ +2    1     u8        interface          [X]
 │ +3    1     u8        flags              [X] Bit 0 = freeze protect
 │ +4    1     u8        color_set          [X]
 │ +5    1     u8        color_pos          [X]
 │ +6    1     u8        color_stagger      [X]
 │ +7    1     u8        device_id          [X]
 │ +8    2     u16 LE    default_runtime    [V] Minutes (720 = 12hrs observed)
 │ +10   2     ???       unknown            [?] 2 bytes, always 0?
 └──────────────────────────────────────────────
 Total per circuit after name: 12 bytes

 After all circuits:
 ...     4     i32 LE    color_count        [V]     8

 Per color (×8):
 ┌──────  ────  ────────  ──────────────── ──────
 │ 0     4+N   SLString  name               [V] e.g., "White", "Blue", "Green"
 │ 4+N'  4     i32 LE    red                [V] Masked to u8 (& 0xFF)
 │ +4    4     i32 LE    green              [V]
 │ +8    4     i32 LE    blue               [V]
 └──────────────────────────────────────────────

 After all colors:
 ...     8     u8[8]     pump_circ_array    [X] 8 bytes, pump-to-circuit mapping
 ...     4     i32 LE    interface_tab_flags [X]
 ...     4     i32 LE    show_alarms        [X]
```

**Gaps:**
- `[?]` 2 unknown bytes after each circuit's default_runtime — always zero?
- `[?]` What do interface_tab_flags bits mean?
- `[?]` What does show_alarms encode?
- `[?]` pump_circ_array — which pump maps to which circuit? Index = pump number, value = circuit?
- `[?]` controller_data byte — what do its bits mean? Top 2 bits = expansion count per screenlogicpy.
- `[?]` name_index normalization: `< 101 ? +1 : +99` — what's the name table?
- `[?]` What does `interface` field mean for each circuit?

---

## 5. EQUIPMENT STATE / STATUS (action 12526 → response 12527)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    param            [V] Always 0
```

### Response (184 bytes observed)

```
 Offset  Size  Type      Field              Status  Live Value
 ──────  ────  ────────  ────────────────   ──────  ──────────
 0       4     i32 LE    panel_mode         [V]     1
 4       1     u8        freeze_mode        [V]     0 (extract bit 3: & 0x08)
 5       1     u8        remotes            [V]     0
 6       1     u8        pool_delay         [V]     0
 7       1     u8        spa_delay          [V]     0
 8       1     u8        cleaner_delay      [V]     0
 9       1     u8        unknown_1          [?]     ?
 10      1     u8        unknown_2          [?]     ?
 11      1     u8        unknown_3          [?]     ?
 12      4     i32 LE    air_temp           [V]     105
 16      4     i32 LE    body_count         [V]     2 (clamped to max 2)

 Per body (×body_count, 24 bytes each):
 ┌──────  ────  ────────  ────────────────   ──────
 │ 0     4     i32 LE    body_type          [V] 0=Pool, 1=Spa
 │ 4     4     i32 LE    current_temp       [V]
 │ 8     4     i32 LE    heat_status        [V] 0=Off, 1=Solar, 2=Heater, 3=Both
 │ 12    4     i32 LE    set_point          [V]
 │ 16    4     i32 LE    cool_set_point     [V]
 │ 20    4     i32 LE    heat_mode          [V] 0=Off, 1=Solar, 2=SolarPref, 3=HeatPump
 └──────────────────────────────────────────────

 ...     4     i32 LE    circuit_count      [V]

 Per circuit (×circuit_count, 12 bytes each):
 ┌──────  ────  ────────  ────────────────   ──────
 │ 0     4     i32 LE    circuit_id         [V] Subtract 499 for logical ID
 │ 4     4     i32 LE    state              [V] 0=Off, 1=On
 │ 8     1     u8        color_set          [X]
 │ 9     1     u8        color_pos          [X]
 │ 10    1     u8        color_stagger      [X]
 │ 11    1     u8        delay              [X]
 └──────────────────────────────────────────────

 After all circuits — inline chemistry:
 ...     4     i32 LE    ph                 [X] Divide by 100
 ...     4     i32 LE    orp                [X] mV
 ...     4     i32 LE    saturation         [X] Divide by 100
 ...     4     i32 LE    salt_ppm           [X] Multiply by 50
 ...     4     i32 LE    ph_tank            [X]
 ...     4     i32 LE    orp_tank           [X]
 ...     4     i32 LE    alarms             [X]
```

**Gaps:**
- `[?]` 3 unknown bytes at offsets 9-11 — what do they encode? Related to freeze_mode byte?
- `[?]` panel_mode — what values are possible? Only seen 1.
- `[?]` remotes byte — what does it indicate?
- `[~]` freeze_mode — bit 3 is freeze per screenlogicpy; what are bits 0-2, 4-7?
- `[?]` Are there additional fields after the inline chemistry data? (Our 184 bytes may have more)
- `[?]` circuit_count in status was 59 in one test but only 7 defined circuits — are the extras virtual/internal circuits?

---

## 6. CHEMISTRY DATA (action 12592 → response 12593)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_idx   [V] Always 0
```

### Response (48 bytes observed — all zeros except sentinel since no IntelliChem installed)

```
 Offset  Size  Type      Field              Status  Notes
 ──────  ────  ────────  ────────────────   ──────  ─────
 0       4     u32 LE    sentinel           [V]     Must be 42 for valid data
 4       1     u8        unknown_04         [?]     Always 0?
 5       2     u16 BE    ph                 [X]     ÷100 for display
 7       2     u16 BE    orp                [X]     mV
 9       2     u16 BE    ph_set_point       [X]     ÷100
 11      2     u16 BE    orp_set_point      [X]
 13      4     u32 BE    ph_dose_time       [X]     Seconds (screenlogicpy decoded)
 17      4     u32 BE    orp_dose_time      [X]     Seconds (screenlogicpy decoded)
 21      2     u16 BE    ph_dose_volume     [X]     mL (screenlogicpy decoded)
 23      2     u16 BE    orp_dose_volume    [X]     mL (screenlogicpy decoded)
 25      1     u8        ph_supply_level    [X]     Tank level
 26      1     u8        orp_supply_level   [X]     Tank level
 27      1     u8        saturation         [X]     Signed: if &0x80, negate. ÷100
 28      2     u16 BE    calcium            [X]
 30      2     u16 BE    cyanuric_acid      [X]
 32      2     u16 BE    alkalinity         [X]
 34      1     u8        salt               [D]     ×50 for PPM. screenlogicpy=u8, node=u16LE
 35      1     u8        probe_is_celsius   [N]     0=F, 1=C
 36      1     u8        water_temp         [X]
 37      1     u8        alarms             [N]     Bitfield (see below)
 38      1     u8        alerts             [N]     Bitfield (see below)
 39      1     u8        dose_flags         [N]     Bits 4-5=pH state, 6-7=ORP state
 40      1     u8        config_flags       [N]     (see below)
 41      1     u8        fw_minor           [N]     IntelliChem firmware
 42      1     u8        fw_major           [N]     IntelliChem firmware
 43      1     u8        balance_flags      [X]     Bit 0=corrosive, bit 1=scaling
 44      1     u8        unknown_44         [?]
 45      1     u8        unknown_45         [?]
 46      1     u8        unknown_46         [?]
```

Total: 47 bytes of parsed fields + sentinel check. Observed 48 bytes.

#### Alarm bits (offset 37)

```
 Bit  Flag                Status
 ───  ──────────────────  ──────
 0    Flow alarm          [N]
 1    pH high             [N]
 2    pH low              [N]
 3    ORP high            [N]
 4    ORP low             [N]
 5    pH supply low       [N]
 6    ORP supply low      [N]
 7    Probe fault         [N]
```

#### Alert bits (offset 38)

```
 Bit  Flag                Status
 ───  ──────────────────  ──────
 0    pH lockout          [N]
 1    pH dose limit       [N]
 2    ORP dose limit      [N]
 3    Invalid setup       [?]
 4    Chlorinator comm    [?]
 5-7  ???                 [?]
```

#### Dose state (offset 39)

```
 Bits  Field              Values                    Status
 ────  ───────────────    ────────────────────────  ──────
 4-5   pH dose state      0=Dosing,1=Mixing,2=Mon   [N]
 6-7   ORP dose state     0=Dosing,1=Mixing,2=Mon   [N]
 0-3   ???                                           [?]
```

#### Config flags (offset 40)

```
 Bit  Flag                Status
 ───  ──────────────────  ──────
 0    ???                 [?]
 1    IntelliChlor        [?]
 2    pH priority         [?]
 3    Use chlorinator     [?]
 4    Advanced display    [?]
 5    pH supply type      [?]
 6    Comms lost          [?]
 7    ???                 [?]
```

**Gaps:**
- `[D]` Salt field at offset 34: u8 (screenlogicpy) vs u16 LE (node-screenlogic). Cannot resolve with all-zero test data. Need IntelliChem or IntelliChlor with known salt level.
- `[?]` 3 unknown trailing bytes at offsets 44-46
- `[?]` Most alarm/alert/config flag bits are only documented by screenlogicpy variable names, not verified
- `[?]` Byte at offset 4 — always 0? What does it mean?

---

## 7. SCG / CHLORINATOR (action 12572 → response 12573)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_idx   [V] Always 0
```

### Response (all i32 LE)

```
 Offset  Size  Type      Field              Status  Notes
 ──────  ────  ────────  ────────────────   ──────  ─────
 0       4     i32 LE    installed          [X]     1=yes, 0=no
 4       4     i32 LE    status             [X]
 8       4     i32 LE    pool_set_point     [X]     % output
 12      4     i32 LE    spa_set_point      [X]     % output
 16      4     i32 LE    salt               [X]     ×50 for PPM
 20      4     i32 LE    flags              [X]
 24      4     i32 LE    super_chlor_timer  [X]
```

**Gaps:**
- `[?]` status field — what values? Bitmask or enum?
- `[?]` flags field — what bits mean what?
- `[?]` Is there data after super_chlor_timer? (Response length not captured)

---

## 8. PUMP STATUS (action 12584 → response 12585)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_idx   [V] Always 0
 4       4     i32 LE    pump_index       [V] 0-indexed (pump 1 = index 0)
```

### Response (all u32 LE)

```
 Offset  Size  Type      Field              Status  Notes
 ──────  ────  ────────  ────────────────   ──────  ─────
 0       4     u32 LE    pump_type          [X]     0=None, 1=VF, 2=VS, 3=VSF
 4       4     u32 LE    is_running         [X]     0=off, 1 or 0xFFFFFFFF=on
 8       4     u32 LE    watts              [X]
 12      4     u32 LE    rpm                [X]
 16      4     u32 LE    unknown_1          [X]     Always 0
 20      4     u32 LE    gpm                [X]
 24      4     u32 LE    unknown_2          [X]     Always 255
```

Then 8 pump circuit presets:

```
 Per preset (×8, 12 bytes each):
 ┌──────  ────  ────────  ────────────────   ──────
 │ 0     4     u32 LE    circuit_id         [X]
 │ 4     4     u32 LE    speed              [X]     RPM or GPM value
 │ 8     4     u32 LE    is_rpm             [X]     nonzero=RPM, 0=GPM
 └──────────────────────────────────────────────
```

Total response: 28 + (8 × 12) = 124 bytes.

**Gaps:**
- `[?]` unknown_1 at offset 16 — always 0. What is this field?
- `[?]` unknown_2 at offset 24 — always 255. What is this field?
- `[?]` is_running: can it be values other than 0, 1, or 0xFFFFFFFF?
- `[?]` Are pump circuit_ids wire-offset (+499) or logical?
- `[?]` screenlogicpy filters pump state with `& 0x80000000` — when does high bit get set?

---

## 9. SCHEDULE DATA (action 12542 → response 12543)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    param            [N] Always 0
 4       4     i32 LE    schedule_type    [X] 0=Recurring, 1=RunOnce
```

### Response

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     u32 LE    event_count      [N]

 Per event (×event_count, 32 bytes each):
 ┌──────  ────  ────────  ────────────────   ──────
 │ 0     4     u32 LE    schedule_id        [X] Subtract 699
 │ 4     4     u32 LE    circuit_id         [X] Subtract 499
 │ 8     4     u32 LE    start_time         [X] Minutes from midnight
 │ 12    4     u32 LE    stop_time          [X] Minutes from midnight
 │ 16    4     u32 LE    day_mask           [X] Mon=0x01..Sun=0x40
 │ 20    4     u32 LE    flags              [?]
 │ 24    4     u32 LE    heat_cmd           [?]
 │ 28    4     u32 LE    heat_set_point     [X]
 └──────────────────────────────────────────────
```

**Gaps:**
- `[?]` flags field — what bits? Only "default 2" in protocol PDF.
- `[?]` heat_cmd field — what values? "default 4" in protocol PDF. Is this HeatMode enum?
- `[?]` No live schedule data captured — need to test against real schedules
- `[?]` How does RunOnce (egg timer) schedule differ from Recurring in the response?
- `[?]` Can a schedule have no stop_time? (For egg timer, is stop_time meaningful?)

---

## 10. BUTTON PRESS / CIRCUIT CONTROL (action 12530 → response 12531)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    circuit_id       [X] Logical + 499
 8       4     i32 LE    state            [X] 0=Off, 1=On
```

### Response

```
 (ack, no meaningful payload)                         [N]
```

---

## 11. HEAT CONTROL

### 11.1 Set Heat Set Point (action 12528 → response 12529)

```
 REQUEST:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    body_type        [X] 0=Pool, 1=Spa
 8       4     i32 LE    temperature      [X]
```

### 11.2 Set Heat Mode (action 12538 → response 12539)

```
 REQUEST:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    body_type        [X] 0=Pool, 1=Spa
 8       4     i32 LE    heat_mode        [X] 0=Off, 1=Solar, 2=SolarPref, 3=HeatPump
```

### 11.3 Set Cool Set Point (action 12590 → response 12591)

```
 REQUEST:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    body_type        [X] 0=Pool, 1=Spa
 8       4     i32 LE    temperature      [X]
```

---

## 12. LIGHT CONTROL (action 12556 → response 12557)

### Request

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    command          [X] See table below
```

### IntelliBrite Light Commands

```
 Value  Command       Status
 ─────  ────────────  ──────
 0      Off           [X]
 1      On            [X]
 2      Set           [X]
 3      Sync          [X]
 4      Swim          [X]
 5      Party         [X]
 6      Romantic      [X]
 7      Caribbean     [X]
 8      American      [X]
 9      Sunset        [X]
 10     Royal         [X]
 11     Save          [X]
 12     Recall        [X]
 13     Blue          [X]
 14     Green         [X]
 15     Red           [X]
 16     White         [X]
 17     Purple        [X]
 18     Thumper       [?] In protocol PDF but not in node-screenlogic enum
 19     Next Mode     [?] In protocol PDF but not in node-screenlogic enum
 20     Reset         [?] In protocol PDF but not in node-screenlogic enum
 21     Hold          [?] In protocol PDF but not in node-screenlogic enum
```

**Gaps:**
- `[?]` Commands 18-21 — are they valid for IntelliBrite? Only in protocol PDF.
- `[?]` Do different light types (SAm, SAL, Color Wheel) use different command sets?

---

## 13. COLOR UPDATE PUSH (action 12504, unsolicited)

```
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    mode             [N]
 4       4     i32 LE    progress         [N]
 8       4     i32 LE    limit            [N]
 12      4+N   SLString  text             [N]
```

**Gaps:**
- `[?]` What values does mode take?
- `[?]` What do progress/limit represent? Animation frame / total frames?

---

## 14. SET SCHEDULE (action 12548 → response 12549)

```
 REQUEST:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    param            [X] Always 0
 4       4     i32 LE    schedule_id      [X] Logical + 699
 8       4     i32 LE    circuit_id       [X] Logical + 499
 12      4     i32 LE    start_time       [X] Minutes from midnight
 16      4     i32 LE    stop_time        [X] Minutes from midnight
 20      4     i32 LE    day_mask         [X] Mon=0x01..Sun=0x40
 24      4     i32 LE    flags            [?] Default 2
 28      4     i32 LE    heat_cmd         [?] Default 4
 32      4     i32 LE    heat_set_point   [X]
```

---

## 15. SET SCG OUTPUT (action 12576 → response 12577)

```
 REQUEST:
 Offset  Size  Type      Field            Status
 ──────  ────  ────────  ───────────────  ──────
 0       4     i32 LE    controller_id    [X] Use 0
 4       4     i32 LE    pool_output      [X] Percentage
 8       4     i32 LE    spa_output       [X] Percentage
 12      4     i32 LE    reserved_1       [X] Always 0
 16      4     i32 LE    reserved_2       [X] Always 0
```

---

## 16. WEATHER FORECAST (action 9807 → response 9808)

### Response

```
 Offset  Size  Type      Field              Status
 ──────  ────  ────────  ────────────────   ──────
 0       4     i32 LE    version            [N]
 4       4+N   SLString  zip_code           [N]
 ...     16    SLDateTime last_update       [N]
 ...     16    SLDateTime last_request      [N]
 ...     4+N   SLString  date_text          [N]
 ...     4+N   SLString  description        [N]
 ...     4     i32 LE    current_temp       [N]
 ...     4     i32 LE    humidity           [N]
 ...     4+N   SLString  wind               [N]
 ...     4     i32 LE    pressure           [N]
 ...     4     i32 LE    dew_point          [N]
 ...     4     i32 LE    wind_chill         [N]
 ...     4     i32 LE    visibility         [N]
 ...     4     i32 LE    num_days           [N]

 Per forecast day (×num_days):
 ┌──────  ────  ────────  ────────────────   ──────
 │ 0     16    SLDateTime day_time          [N]
 │ 16    4     i32 LE    high_temp          [N]
 │ 20    4     i32 LE    low_temp           [N]
 │ 24    4+N   SLString  text               [N]
 └──────────────────────────────────────────────

 After all days:
 ...     4     i32 LE    sunrise            [N] Minutes from midnight?
 ...     4     i32 LE    sunset             [N] Minutes from midnight?
```

**Gaps:**
- `[?]` Where does the weather data come from? Pentair cloud? Zip code lookup?
- `[?]` sunrise/sunset format — minutes from midnight or something else?
- `[?]` Is this still functional or deprecated?

---

## 17. SYSTEM TIME (action 8110 → response 8111)

```
 RESPONSE:
 Offset  Size  Type       Field              Status
 ──────  ────  ────────── ────────────────   ──────
 0       16    SLDateTime  system_time       [N]
 16      4     i32 LE     adjust_for_dst     [N] 1=true
```

---

## 18. HISTORY DATA (action 12534 → response 12535)

### Request

```
 Offset  Size  Type       Field            Status
 ──────  ────  ────────── ───────────────  ──────
 0       4     i32 LE     controller_idx   [N] Use 0
 4       16    SLDateTime start_time       [N]
 20      16    SLDateTime end_time         [N]
 36      4     i32 LE     sender_id        [N]
```

### Response — arrays of time-value pairs

```
 1. airTemps:       count(i32) × [SLDateTime(16) + temp(i32)]     [N]
 2. poolTemps:      count(i32) × [SLDateTime(16) + temp(i32)]     [N]
 3. poolSetPoints:  count(i32) × [SLDateTime(16) + temp(i32)]     [N]
 4. spaTemps:       count(i32) × [SLDateTime(16) + temp(i32)]     [N]
 5. spaSetPoints:   count(i32) × [SLDateTime(16) + temp(i32)]     [N]
 6. poolRuns:       count(i32) × [SLDateTime(16) + SLDateTime(16)] [N]
 7. spaRuns:        count(i32) × [SLDateTime(16) + SLDateTime(16)] [N]
 8. solarRuns:      count(i32) × [SLDateTime(16) + SLDateTime(16)] [N]
 9. heaterRuns:     count(i32) × [SLDateTime(16) + SLDateTime(16)] [N]
 10. lightRuns:     count(i32) × [SLDateTime(16) + SLDateTime(16)] [N]
```

---

## 19. EQUIPMENT CONFIGURATION (action 12566 → response 12567)

The "detailed" equipment config with raw data arrays.

```
 Offset  Size  Type      Field                Status
 ──────  ────  ────────  ──────────────────── ──────
 0       1     u8        controller_type      [N]
 1       1     u8        hardware_type        [N]
 2       1     u8        unknown              [?]
 3       1     u8        unknown              [?]
 4       4     u32 LE    controller_data      [N]
 8       4+N   SLArray   version_data         [N] 16 bytes observed
 ...     4+N   SLArray   speed_data           [N] 8 bytes (high speed circuits)
 ...     4+N   SLArray   valve_data           [N] 24 bytes
 ...     4+N   SLArray   remote_data          [N] 44 bytes
 ...     4+N   SLArray   sensor_data          [N] 3 bytes + 1 pad
 ...     4+N   SLArray   delay_data           [N] 2 bytes + 2 pad
 ...     4+N   SLArray   macros_data          [N] 140 bytes
 ...     4+N   SLArray   misc_data            [N] 10 bytes + 2 pad
 ...     4+N   SLArray   light_data           [N] 12 bytes
 ...     4+N   SLArray   flows_data           [N] 360 bytes (pump config)
 ...     4+N   SLArray   sgs_data             [N] 22 bytes + 2 pad
 ...     4+N   SLArray   spa_flows_data       [N] 16 bytes
```

**Gaps — nearly everything in the sub-arrays is undocumented:**

#### version_data (16 bytes)
- `[N]` Byte 0: major version, byte 1: minor? (`version = byte[0]*1000 + byte[1]`)
- `[?]` Bytes 2-15: unknown

#### speed_data (8 bytes)
- `[?]` High speed circuit config — which bytes map to what?

#### valve_data (24 bytes)
- `[~]` Per-valve config. screenlogicpy partially decodes.

#### sensor_data (3 bytes)
- `[~]` Heater config bits: byte[0] bit 1 = solar present, byte[2] bit 4 = heat pump present

#### delay_data (2 bytes)
- `[~]` byte[0] bit 0 = pool pump on during heater cooldown
- `[~]` byte[0] bit 1 = spa pump on during heater cooldown
- `[~]` byte[0] bit 7 = pump off during valve action

#### misc_data (10 bytes)
- `[~]` byte[3] bit 0 = intelliChem installed
- `[~]` byte[4] = manual heat mode (nonzero = manual)
- `[?]` Bytes 0-2, 5-9: unknown

#### flows_data (360 bytes)
- `[~]` Pump configuration in 45-byte chunks (8 pumps × 45 bytes)
- `[~]` At offset `45*i + 2`: pump type (0=none, 1=VF, 2=VS, 3=VSF)
- `[?]` Remaining bytes per pump: circuit assignments, speed settings?

#### light_data (12 bytes)
- `[?]` IntelliBrite position/color config?

#### sgs_data (22 bytes)
- `[?]` Salt generator settings?

#### spa_flows_data (16 bytes)
- `[?]` Spa-specific flow settings?

---

## 20. MESSAGES NEVER CAPTURED OR TESTED

These action codes exist in the protocol appendix but have no implementation details from either reference:

```
 Action   Name                          Status
 ──────   ────────────────────────────  ──────
 12506    Chem History Data             [N] Exists but undocumented format
 12510    Get Circuit Definitions       [N] "Appears to be unused" per PDF
 12518    Get Circuit Info By ID        [N]
 12520    Set Circuit Info By ID        [N]
 12544    Add New Scheduled Event       [X] Payload: i32 0, i32 schedType
 12558    Get N Circuits                [N]
 12560    Get Circuit Names             [X] Partially decoded
 12564    Set Custom Name               [X]
 12568    Set Equipment Configuration   [~] Complex, many sub-arrays
 12570    Set Cal                       [N]
 12574    Set SCG Enabled               [X]
 12578    Enable Remotes                [N]
 12580    Cancel Delays                 [X]
 12586    Set Pump Flow                 [X]
 12588    Reset House Code              [N]
 12594    Set Chem Data                 [N]
 12596    Get Chem History Data         [N]
 8058     Firmware Query                [?] Alternative version query per screenlogicpy
```

---

## 21. FULL ACTION CODE MAP

```
 Code    Request (Q)                    Response (A)                  Status
 ──────  ─────────────────────────────  ────────────────────────────  ──────
 13      --                             Login Failure                 [V]
 14/15   Challenge                      Challenge Response            [N]
 16/17   Ping                           Pong                          [N]
 27/28   Login                          Login Accepted                [V]
 30      --                             Unknown Command               [N]
 31      --                             Bad Parameter                 [N]
 8110/11 Get System Time                System Time                   [N]
 8112/13 Set System Time                Set Time Ack                  [N]
 8120/21 Get Version                    Version String                [V]
 9806    --                             Weather Forecast Changed      [N]
 9807/08 Get Weather                    Weather Forecast              [N]
 12500   --                             Status Changed (push)         [N]
 12501   --                             Schedule Changed (push)       [N]
 12502   --                             History Data (push)           [N]
 12503   --                             Runtime Changed (push)        [N]
 12504   --                             Color Update (push)           [N]
 12505   --                             Chemistry Changed (push)      [N]
 12522/23 Add Client                    Add Client Ack                [N]
 12524/25 Remove Client                 Remove Client Ack             [N]
 12526/27 Get Status                    Equipment State               [V]
 12528/29 Set Heat Set Point            Set Heat SP Ack               [N]
 12530/31 Button Press                  Button Press Ack              [N]
 12532/33 Get Controller Config         Controller Config             [V]
 12534/35 Get History                   History Data                  [N]
 12538/39 Set Heat Mode                 Set Heat Mode Ack             [N]
 12542/43 Get Schedule Data             Schedule Data                 [N]
 12544/45 Add Schedule Event            Add Schedule Ack              [N]
 12546/47 Delete Schedule Event         Delete Schedule Ack           [N]
 12548/49 Set Schedule Event            Set Schedule Ack              [N]
 12550/51 Set Circuit Runtime           Set Runtime Ack               [N]
 12556/57 Color Lights Command          Color Lights Ack              [N]
 12560/61 Get Circuit Names             Circuit Names                 [N]
 12562/63 Get Custom Names              Custom Names                  [N]
 12564/65 Set Custom Name               Set Custom Name Ack           [N]
 12566/67 Get Equipment Config          Equipment Config              [N]
 12568/69 Set Equipment Config          Set Equipment Config Ack      [N]
 12572/73 Get SCG Config                SCG Config                    [V]
 12574/75 Set SCG Enabled               Set SCG Enabled Ack           [N]
 12576/77 Set SCG Config                Set SCG Config Ack            [N]
 12580/81 Cancel Delay                  Cancel Delay Ack              [N]
 12582/83 Get All Errors                All Errors                    [N]
 12584/85 Get Pump Status               Pump Status                   [V]
 12586/87 Set Pump Speed                Set Pump Speed Ack            [N]
 12590/91 Set Cool Set Point            Set Cool SP Ack               [N]
 12592/93 Get Chem Data                 Chem Data                     [V]
 12596/97 Get Chem History              Chem History                  [N]
 18003/04 Gateway Request               Gateway Response              [N]
```

Legend: `[V]` = verified on live hardware, `[N]` = not tested, `[X]` = cross-checked sources only

---

## 22. OBSERVATION LOG — ANOMALIES

Things noticed during live testing that don't fit neatly above:

1. **Circuit count mismatch**: Controller config shows 7 circuits, but status response reported 59 circuits. The extra circuits have IDs like 100, 103, 71. These appear to be system/virtual circuits (features, aux, etc.) that don't map to physical relay outputs.

2. **Air temp 105°F**: This was the actual air temperature during testing (Arizona, summer). Confirms the value is real, not a protocol artifact.

3. **All chem data zeros**: Confirmed no IntelliChem installed (not in equipment flags). The sentinel=42 still present means the response structure is valid even without the hardware.

4. **Login response 16 bytes**: The successful login response (action 28) contained 16 bytes of payload, all zeros. Purpose unknown.

5. **Discovery response 40 bytes**: Longer than the 12 bytes documented in the protocol PDF. Extra 28 bytes contain the adapter name string.
