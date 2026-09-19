import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockInvoke, usingMocks } from "./devMocks";
import type {AppConfig, AppError, AppInfo, AppPrefs, AudioDeviceInfo, ConfigPayload, DeviceEntry, DeviceTypeInfo, EngineStatus, HueLightInfo, LanguageSetting, LayoutSelector, LogLine, SpeakerPlacement, Theme,} from "./bindings";

export const EVENT_STATUS = "engine://status";
export const EVENT_LOG = "engine://log";

const invoke: typeof tauriInvoke = usingMocks ? (mockInvoke as typeof tauriInvoke) : tauriInvoke;
const listen: typeof tauriListen = usingMocks ? ((() => Promise.resolve(() => {})) as unknown as typeof tauriListen) : tauriListen;

export const api = {
  loadConfig: () => invoke<ConfigPayload>("load_config"),
  saveConfig: (config: AppConfig) => invoke<string[]>("save_config", { config }),
  configPath: () => invoke<string>("config_path"),
  setConfigPath: (path: string, moveFile: boolean) => invoke<string>("set_config_path", { path, moveFile }),

  engineStatus: () => invoke<EngineStatus>("engine_status"),
  engineStart: () => invoke<void>("engine_start"),
  engineStop: () => invoke<void>("engine_stop"),
  engineReload: () => invoke<void>("engine_reload"),

  listAudioDevices: () => invoke<AudioDeviceInfo[]>("list_audio_devices"),
  listAudioApps: () => invoke<AppInfo[]>("list_audio_apps"),
  deviceTypes: () => invoke<DeviceTypeInfo[]>("device_types"),
  resolveSpeakers: (layout: LayoutSelector, channels?: number) => invoke<SpeakerPlacement[]>("resolve_speakers", { layout, channels }),
  testDevice: (entry: DeviceEntry) => invoke<string>("test_device", { entry }),

  huePair: (bridge: string) => invoke<string>("hue_pair", { bridge }),
  hueLights: (bridge: string, applicationKey: string) => invoke<HueLightInfo[]>("hue_lights", { bridge, applicationKey }),

  recentLogs: () => invoke<LogLine[]>("recent_logs"),
  getPrefs: () => invoke<AppPrefs>("get_prefs"),
  setTheme: (theme: Theme) => invoke<void>("set_theme", { theme }),
  setLanguage: (setting: LanguageSetting, effective: Exclude<LanguageSetting, "system">) => invoke<void>("set_language", { setting, effective }),
};

export function onStatus(handler: (status: EngineStatus) => void): Promise<UnlistenFn> {return listen<EngineStatus>(EVENT_STATUS, (event) => handler(event.payload));}
export function onLog(handler: (line: LogLine) => void): Promise<UnlistenFn> {return listen<LogLine>(EVENT_LOG, (event) => handler(event.payload));}
export function asAppError(e: unknown): AppError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    return e as AppError;
  }
  return { kind: "engine", message: String(e) };
}
