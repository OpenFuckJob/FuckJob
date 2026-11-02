/**
 * Capture document.cookie writes and XMLHttpRequest request headers.
 * Inject before page load when possible.
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

    try {
        const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie') ||
            Object.getOwnPropertyDescriptor(HTMLDocument.prototype, 'cookie');
        if (descriptor && descriptor.configurable && !descriptor.set.__webReverseHooked) {
            const nativeSet = descriptor.set;
            const nativeGet = descriptor.get;
            const hookedSet = function (value) {
                push('cookie:set', { value: String(value) });
                return nativeSet.call(this, value);
            };
            hookedSet.__webReverseHooked = true;
            Object.defineProperty(document, 'cookie', {
                configurable: true,
                enumerable: true,
                get: function () { return nativeGet.call(document); },
                set: hookedSet,
            });
        }
    } catch (error) {
        push('cookie:hook-error', { message: String(error && error.message ? error.message : error) });
    }

    if (root.XMLHttpRequest && !root.XMLHttpRequest.prototype.__webReverseHeaderHooked) {
        const proto = root.XMLHttpRequest.prototype;
        const nativeSetRequestHeader = proto.setRequestHeader;
        proto.setRequestHeader = function (name, value) {
            this.__webReverseRequest = this.__webReverseRequest || { headers: {} };
            this.__webReverseRequest.headers = this.__webReverseRequest.headers || {};
            this.__webReverseRequest.headers[String(name)] = String(value);
            push('xhr:header', { name: String(name), value: String(value) });
            return nativeSetRequestHeader.apply(this, arguments);
        };
        proto.__webReverseHeaderHooked = true;
    }
})();
