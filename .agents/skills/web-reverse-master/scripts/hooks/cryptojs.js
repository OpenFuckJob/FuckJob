/**
 * Capture common CryptoJS calls. Inject before the code under analysis creates
 * closed-over references, or rerun after CryptoJS becomes available.
 */

(function () {
    const root = typeof window !== 'undefined' ? window : globalThis;
    const logs = root.__webReverseLogs = root.__webReverseLogs || [];

    function push(type, payload) {
        logs.push({ type, ts: Date.now(), payload });
        if (root.console && root.console.debug) {
            root.console.debug('[web-reverse]', type, payload);
        }
    }

    function preview(value) {
        if (value == null) return null;
        try {
            return String(value).slice(0, 500);
        } catch (error) {
            return Object.prototype.toString.call(value);
        }
    }

    function wrapFunction(owner, name, type) {
        if (!owner || typeof owner[name] !== 'function' || owner[name].__webReverseHooked) return;
        const nativeFn = owner[name];
        const hooked = function () {
            const args = Array.prototype.slice.call(arguments).map(preview);
            const result = nativeFn.apply(this, arguments);
            push(type, {
                args,
                result: preview(result),
            });
            return result;
        };
        hooked.__webReverseHooked = true;
        hooked.toString = function () { return nativeFn.toString(); };
        owner[name] = hooked;
    }

    function install() {
        const CryptoJS = root.CryptoJS;
        if (!CryptoJS) {
            push('cryptojs:missing', { message: 'CryptoJS is not defined yet' });
            return false;
        }

        ['MD5', 'SHA1', 'SHA256', 'SHA512', 'HmacSHA256'].forEach(function (name) {
            wrapFunction(CryptoJS, name, 'cryptojs:' + name);
        });

        ['AES', 'DES', 'TripleDES', 'RC4'].forEach(function (family) {
            if (CryptoJS[family]) {
                wrapFunction(CryptoJS[family], 'encrypt', 'cryptojs:' + family + '.encrypt');
                wrapFunction(CryptoJS[family], 'decrypt', 'cryptojs:' + family + '.decrypt');
            }
        });

        return true;
    }

    root.__installWebReverseCryptoHooks = install;
    install();
})();
