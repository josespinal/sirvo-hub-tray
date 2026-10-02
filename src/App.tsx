import { getCurrentWindow } from "@tauri-apps/api/window";
import LogsWindow from "./LogsWindow.js";
import SetupWindow from "./SetupWindow.js";

// One bundle serves every window; the window's label picks the screen.
export default function App() {
  return getCurrentWindow().label === "setup" ? <SetupWindow /> : <LogsWindow />;
}
