export type OverlayStatus = {visible:boolean;phase:string;message:string;recordingMs:number;stageMs:number;level:number};
type Elements = {root:HTMLElement;phase:HTMLElement;timing:HTMLElement;meter:HTMLElement;fill:HTMLElement};
export function setupOverlayView(elements:Elements,read:()=>Promise<OverlayStatus>) {
  let reading=false;
  const text=(element:HTMLElement,value:string)=>{if(element.textContent!==value)element.textContent=value;};
  function render(status:OverlayStatus) {
    elements.root.hidden=!status.visible;
    elements.root.dataset.phase=status.phase;
    text(elements.phase,status.message);
    const seconds=(ms:number)=>`${(Math.max(0,ms)/1000).toFixed(1)}秒`;
    text(elements.timing,`録音 ${seconds(status.recordingMs)}${status.phase==="correcting"?` / 補正 ${seconds(status.stageMs)}`:status.phase==="recognizing"?` / 認識 ${seconds(status.stageMs)}`:""}`);
    const rms=status.visible && status.phase==="recording" && Number.isFinite(status.level)?Math.min(1,Math.max(0,status.level)):0;
    const db=rms>0?20*Math.log10(rms):-Infinity;
    const value=Math.max(0,(Math.max(-60,db)+60)/60);
    elements.fill.style.width=`${value*100}%`;
    elements.meter.setAttribute("aria-valuenow",String(value));
    elements.meter.setAttribute("aria-valuetext",`${Number.isFinite(db)?db.toFixed(1):"-∞"} dBFS`);
  }
  async function poll() {
    if(reading)return;
    reading=true;
    try {render(await read());}
    catch {render({visible:true,phase:"error",message:"状態を確認できません。設定画面を確認してください。",recordingMs:0,stageMs:0,level:0});}
    finally {reading=false;}
  }
  return {poll};
}
