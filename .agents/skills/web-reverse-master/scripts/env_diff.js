/**
 * Browser environment capture for env patching diagnostics.
 *
 * Browser Console:
 *   copy/paste this file and save the printed JSON as browser_env.json
 *
 * Node or patched runtime:
 *   node scripts/env_diff.js > patched_env.json
 */

(function () {
    function safe(name, get) {
        try {
            const value = get();
            return [name, { value: String(value), type: typeof value }];
        } catch (error) {
            return [name, { error: String(error && error.message ? error.message : error) }];
        }
    }

    function canvasFingerprint() {
        const canvas = document.createElement('canvas');
        canvas.width = 280;
        canvas.height = 60;
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = '#f60';
        ctx.fillRect(0, 0, 280, 60);
        ctx.fillStyle = '#069';
        ctx.font = '24px Arial';
        ctx.fillText('WebReverse', 10, 40);
        return canvas.toDataURL().substring(0, 80);
    }

    function webglVendor() {
        const canvas = document.createElement('canvas');
        const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
        if (!gl) return 'N/A';
        const debugInfo = gl.getExtension('WEBGL_debug_renderer_info');
        if (!debugInfo) return 'N/A';
        return gl.getParameter(debugInfo.UNMASKED_VENDOR_WEBGL);
    }

    function audioSampleRate() {
        const AudioCtor = typeof AudioContext !== 'undefined' ? AudioContext : webkitAudioContext;
        const ctx = new AudioCtor();
        const rate = ctx.sampleRate;
        if (typeof ctx.close === 'function') ctx.close();
        return rate;
    }

    const checks = [
        ['navigator.userAgent', () => navigator.userAgent],
        ['navigator.platform', () => navigator.platform],
        ['navigator.language', () => navigator.language],
        ['navigator.hardwareConcurrency', () => navigator.hardwareConcurrency],
        ['navigator.deviceMemory', () => navigator.deviceMemory],
        ['navigator.webdriver', () => navigator.webdriver],
        ['navigator.plugins.length', () => navigator.plugins.length],
        ['navigator.mimeTypes.length', () => navigator.mimeTypes.length],
        ['navigator.plugins[0]?.name', () => navigator.plugins[0] ? navigator.plugins[0].name : 'N/A'],
        ['navigator.mimeTypes[0]?.type', () => navigator.mimeTypes[0] ? navigator.mimeTypes[0].type : 'N/A'],
        ['document.cookie', () => document.cookie ? document.cookie.substring(0, 80) : ''],
        ['document.hidden', () => document.hidden],
        ['document.visibilityState', () => document.visibilityState],
        ['document.readyState', () => document.readyState],
        ['typeof document.all', () => typeof document.all],
        ['screen.width', () => screen.width],
        ['screen.height', () => screen.height],
        ['screen.colorDepth', () => screen.colorDepth],
        ['screen.pixelDepth', () => screen.pixelDepth],
        ['location.href', () => location.href],
        ['location.protocol', () => location.protocol],
        ['typeof window', () => typeof window],
        ['typeof document', () => typeof document],
        ['typeof navigator', () => typeof navigator],
        ['window.toString()', () => window.toString()],
        ['navigator.toString()', () => navigator.toString ? navigator.toString() : 'N/A'],
        ['alert.toString()', () => window.alert ? window.alert.toString().substring(0, 80) : 'N/A'],
        ['setTimeout.toString()', () => setTimeout.toString().substring(0, 80)],
        ['typeof HTMLCanvasElement', () => typeof HTMLCanvasElement],
        ['canvas.toDataURL()', canvasFingerprint],
        ['WebGL VENDOR', webglVendor],
        ['typeof AudioContext', () => typeof AudioContext],
        ['AudioContext.sampleRate', audioSampleRate],
        ['typeof crypto', () => typeof crypto],
        ['crypto.randomUUID()', () => crypto.randomUUID ? crypto.randomUUID().substring(0, 24) : 'N/A'],
        ['process (Node.js leak)', () => typeof process],
        ['__dirname (Node.js leak)', () => typeof __dirname],
        ['__filename (Node.js leak)', () => typeof __filename],
        ['global (Node.js leak)', () => typeof global],
        ['document.__driver_evaluate', () => typeof document.__driver_evaluate],
        ['document.__webdriver_evaluate', () => typeof document.__webdriver_evaluate],
    ];

    const results = {};
    for (const [name, get] of checks) {
        const [key, value] = safe(name, get);
        results[key] = value;
    }

    if (typeof module !== 'undefined' && module.exports) {
        module.exports = { collectEnv: function () { return results; } };
    }

    if (typeof console !== 'undefined' && console.log) {
        console.log(JSON.stringify(results, null, 2));
    }

    return results;
})();
