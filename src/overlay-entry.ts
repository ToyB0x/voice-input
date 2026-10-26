import {invoke} from "@tauri-apps/api/core";
import {setupOverlayView,type OverlayStatus} from "./overlay-view";
import "./overlay.css";
const element=(id:string)=>document.getElementById(id)!;
const view=setupOverlayView({root:element("overlay"),phase:element("phase"),timing:element("timing"),meter:element("meter"),fill:element("fill")},()=>invoke<OverlayStatus>("overlay_status"));
void view.poll();
window.setInterval(()=>void view.poll(),50);
document.addEventListener("visibilitychange",()=>void view.poll());
