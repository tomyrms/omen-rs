//! BIOS command families and identifiers.

/// Command family: most commands (`Command` property value).
pub const CMD_DEFAULT: u32 = 0x20008;
/// Command family: keyboard-related commands.
pub const CMD_KEYBOARD: u32 = 0x20009;
/// Command family: earliest implemented commands.
pub const CMD_LEGACY: u32 = 0x00001;
/// Command family: graphics mode switch.
pub const CMD_GPU_MODE: u32 = 0x00002;

/// Command identifiers (`CommandType` property values).
pub mod cmd {
    // Capability / system
    pub const GET_SYSTEM_DATA: u32 = 0x28;
    pub const GET_FAN_COUNT: u32 = 0x10;
    pub const GET_FAN_TYPE: u32 = 0x2C;
    pub const GET_KBD_TYPE: u32 = 0x2B;

    // Thermal
    pub const SET_FAN_MODE: u32 = 0x1A;
    pub const SET_FAN_LEVEL: u32 = 0x2E;
    pub const GET_FAN_LEVEL: u32 = 0x2D;
    pub const GET_FAN_TABLE: u32 = 0x2F;
    pub const SET_FAN_TABLE: u32 = 0x32;
    pub const GET_MAX_FAN: u32 = 0x26;
    pub const SET_MAX_FAN: u32 = 0x27;
    pub const GET_TEMPERATURE: u32 = 0x23;
    pub const GET_THROTTLING: u32 = 0x35;

    // Power
    pub const GET_GPU_POWER: u32 = 0x21;
    pub const SET_GPU_POWER: u32 = 0x22;
    pub const SET_CPU_POWER: u32 = 0x29;
    pub const GET_POWER_DATA: u32 = 0x2A;

    // Graphics mode (legacy)
    pub const GET_GPU_MODE: u32 = 0x52;
    pub const SET_GPU_MODE: u32 = 0x52;

    // Keyboard family (0x20009)
    pub const GET_KBD_SUPPORT: u32 = 0x01;
    pub const GET_COLOR_TABLE: u32 = 0x02;
    pub const SET_COLOR_TABLE: u32 = 0x03;
    pub const GET_BACKLIGHT: u32 = 0x04;
    pub const SET_BACKLIGHT: u32 = 0x05;
    pub const GET_ANIM_TABLE: u32 = 0x06;
    pub const SET_ANIM_TABLE: u32 = 0x07;
}
