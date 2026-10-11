import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function AboutSettings() {
  const [metadata, setMetadata] = useState<{ version: string; build: string } | null>(null);
  const [settingError, setSettingError] = useState("");
  useEffect(() => {
    invoke<{ version: string; build: string }>("app_metadata").then(setMetadata).catch(error => setSettingError(String(error)));
  }, []);
  return (
<div className="about-page">
          <section className="surface settings-card" aria-label="版本信息">
            <h2>InkSay</h2>
            <p className="about-description">InkSay 是一款语音输入工具，支持本地与云端识别、文字整理和翻译，可将口述内容直接写入当前应用。</p>
            <div className="settings-row"><span>版本</span><span>{metadata && `${metadata.version}（Build:${metadata.build}）`}</span></div>
          </section>
          <section className="surface settings-card" aria-label="开发者信息">
            <h2>开发者信息</h2>
            <div className="settings-row"><span>公众号</span><span>@唐小羲</span></div>
            <div className="settings-row"><span>小红书</span><a className="about-link" href="https://www.xiaohongshu.com/user/profile/5df9060d0000000001004d95" onClick={(event) => {
              event.preventDefault();
              setSettingError("");
              void invoke("contact_developer", { channel: "xiaohongshu" }).catch((error) => setSettingError(String(error)));
            }}>@唐小羲</a></div>
            <div className="settings-row"><span>邮箱</span><a className="about-link" href="mailto:imseantang@gmail.com" onClick={(event) => {
              event.preventDefault();
              setSettingError("");
              void invoke("contact_developer", { channel: "email" }).catch((error) => setSettingError(String(error)));
            }}>imseantang@gmail.com</a></div>
            {settingError && <div className="settings-error" role="alert">{settingError}</div>}
          </section>
        </div>
  );
}
