// Types for the controller's HTTP API. Everything under ./generated is written by
// `vp run generate` from crates/server (api.rs, models.rs, ...); change the Rust, not the output.

export type { AcPatch } from './generated/AcPatch';
export type { AcPoint } from './generated/AcPoint';
export type { AcState } from './generated/AcState';
export type { ActivityItem } from './generated/ActivityItem';
export type { ActivityKind } from './generated/ActivityKind';
export type { ActivityResponse } from './generated/ActivityResponse';
export type { Command } from './generated/Command';
export type { CommandView } from './generated/CommandView';
export type { ControllerState } from './generated/ControllerState';
export type { DeviceInfo } from './generated/DeviceInfo';
export type { Direction } from './generated/Direction';
export type { Fan } from './generated/Fan';
export type { HistoryResponse } from './generated/HistoryResponse';
export type { Limits } from './generated/Limits';
export type { PauseRequest } from './generated/PauseRequest';
export type { PauseResponse } from './generated/PauseResponse';
export type { Settings } from './generated/Settings';
export type { SettingsPatch } from './generated/SettingsPatch';
export type { StateResponse } from './generated/StateResponse';
export type { Status } from './generated/Status';
export type { ZoneControl } from './generated/ZoneControl';
export type { ZoneEval } from './generated/ZoneEval';
export type { ZonePatch } from './generated/ZonePatch';
export type { ZonePoint } from './generated/ZonePoint';
export type { ZoneReading } from './generated/ZoneReading';
export type { ZoneSeries } from './generated/ZoneSeries';
export type { ZoneState } from './generated/ZoneState';
