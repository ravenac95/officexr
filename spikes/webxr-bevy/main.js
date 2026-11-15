// THROWAWAY spike driver for OSO-7951: Bevy (webgl2) in an emulated WebXR session.
import initWasm, * as r from './pkg/officexr_render_web.js';
import { XRDevice, metaQuest3 } from 'https://cdn.jsdelivr.net/npm/iwer@2.8.0/build/iwer.module.js';

const log = (m) => { document.getElementById('log').textContent += m + '\n'; console.log(m); };
window.__xr = { frames: 0, errors: [] };
try {
  if (!navigator.xr || new URLSearchParams(location.search).has('emulate')) {
    const dev = new XRDevice(metaQuest3, { visibilityMask: null }); dev.installRuntime({ forceInstall: true }); dev.stereoEnabled = true;
    log('IWER runtime installed (metaQuest3)');
  }
  await initWasm(); window.__xrr = r;
  const canvas = document.getElementById('c');
  r.init(location.origin, canvas);
  const view = r.createView(canvas, 'world');
  r.setCamera(view, { position: [0, 1.6, 3], lookAt: [0, 1, -3], fovDeg: 60, near: 0.1, far: 100 });
  r.setWorld(view, JSON.stringify(await (await fetch('./world.json')).json()));
  log('bevy booted, view ' + view);

  window.enterVR = async () => {
    const session = await navigator.xr.requestSession('immersive-vr', { requiredFeatures: ['local-floor'] });
    // The context wgpu's GL backend already made on this canvas: a second getContext returns it.
    const gl = canvas.getContext('webgl2');
    await gl.makeXRCompatible?.();
    const layer = new XRWebGLLayer(session, gl);
    session.updateRenderState({ baseLayer: layer });
    const space = await session.requestReferenceSpace('local-floor');
    log(`session started, layer ${layer.framebufferWidth}x${layer.framebufferHeight}, fb=${layer.framebuffer}`);
    session.addEventListener('end', () => r.xrEnd());
    const onFrame = (_t, frame) => {
      session.requestAnimationFrame(onFrame);
      const pose = frame.getViewerPose(space);
      if (!pose) return;
      const before = r.xrUpdates();
      r.xrFrame({
        view,
        origin: [0, 0, 3],
        yaw: 0,
        framebufferHeight: layer.framebufferHeight,
        eyes: pose.views.slice(0, window.__xr.oneEye ? 1 : 2).map((v) => {
          const vp = layer.getViewport(v);
          return { viewport: [vp.x, vp.y, vp.width, vp.height], transform: Array.from(v.transform.matrix), projection: Array.from(v.projectionMatrix) };
        }),
      });
      // Runs after winit's wake-up microtask, i.e. after Bevy drew this frame into the canvas.
      queueMicrotask(() => {
        window.__xr.frames++; (window.__xr.t ??= []).push(Math.round(performance.now()));
        (window.__xr.inFrame ??= []).push(r.xrUpdates() - before);
        if (window.__xr.frames === 2) window.__xr.eyes = pose.views.map((v) => ({ vp: layer.getViewport(v), t: Array.from(v.transform.matrix).map((n) => +n.toFixed(3)), p: Array.from(v.projectionMatrix).map((n) => +n.toFixed(3)) }));
        if (window.__xr.frames % 5 === 1) {
          const c2 = document.createElement('canvas'); c2.width = canvas.width; c2.height = canvas.height;
          const ctx = c2.getContext('2d'); ctx.drawImage(canvas, 0, 0);
          const px = (x, y) => Array.from(ctx.getImageData(x, y, 1, 1).data);
          window.__xr.probe = { size: [canvas.width, canvas.height, canvas.clientWidth, canvas.clientHeight], row: [50, 300, 450, 700, 900, 1150, 1300, 1550].map((x) => px(x, 400)), upd: r.xrUpdates() };
        }
        if (!layer.framebuffer) return; // emulator: the canvas is the XR surface
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, layer.framebuffer);
        const w = layer.framebufferWidth, h = layer.framebufferHeight;
        gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      });
    };
    session.requestAnimationFrame(onFrame);
    window.__xr.session = session;
  };
  window.__xr.ready = true;
} catch (e) {
  window.__xr.errors.push(String(e?.stack ?? e));
  log('ERROR ' + e);
}
