# InkSay 图标资源

当前应用图标源文件：`inksay-app-blue.svg`，蓝色底色、白色声波；沿用菜单栏声波的比例。
桌面 PNG / ICNS / ICO 位于 `../../src-tauri/icons/`，菜单栏模板为其中的 `tray.png`。

在项目根目录重新生成桌面尺寸：

```sh
npm run tauri -- icon assets/brand/inksay-app-blue.svg
```

侧栏标识由 `src/App.tsx` 内联 SVG 绘制，颜色由样式控制，避免低分辨率位图放大。
本目录旧的白底 PNG 和早期蓝底 PNG 保留作设计参考，不作为当前发布源文件。
