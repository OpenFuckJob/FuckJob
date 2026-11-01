/**
 * Capture fetch and XMLHttpRequest traffic into window.__webReverseLogs.
 * Inject before page load when possible.
 */

(function () {
    const root = typeof window !== 'undefined' ? window : globalThis;
    const logs = root.__webReverseLogs = root.__webReverseLogs || [];

    function push(type, payload) {
        logs.push({
            type,
            ts: Date.now(),
            payload,
        });
        if (root.console && root.console.debug) {
            root.console.debug('[web-reverse]', type, payload);
        }
    }

    if (typeof root.fetch === 'function' && !root.fetch.__webReverseHooked) {
        const nativeFetch = root.fetch;
        const hookedFetch = function () {
            const args = Array.prototype.slice.call(arguments);
            push('fetch:request', {
                input: String(args[0]),
                init: args[1] || null,
            });
            return nativeFetch.apply(this, args).then(function (response) {
                push('fetch:response', {
                    url: response.url,
                    status: response.status,
                    ok: response.ok,
                });
                return response;
            });
        };
        hookedFetch.__webReverseHooked = true;
        hookedFetch.toString = function () { return nativeFetch.toString(); };
        root.fetch = hookedFetch;
    }

    if (root.XMLHttpRequest && !root.XMLHttpRequest.prototype.__webReverseHooked) {
        const proto = root.XMLHttpRequest.prototype;
        const nativeOpen = proto.open;
        const nativeSend = proto.send;

        proto.open = function (method, url) {
            this.__webReverseRequest = {
                method: method,
                url: String(url),
                headers: {},
            };
            return nativeOpen.apply(this, arguments);
        };

        proto.send = function (body) {
            const meta = this.__webReverseRequest || {};
            push('xhr:request', {
                method: meta.method,
                url: meta.url,
                headers: meta.headers || {},
                body: body == null ? null : String(body).slice(0, 2000),
            });
            this.addEventListener('loadend', function () {
                push('xhr:response', {
                    method: meta.method,
                    url: meta.url,
                    status: this.status,
                    responseURL: this.responseURL,
                });
            });
            return nativeSend.apply(this, arguments);
        };

        proto.__webReverseHooked = true;
    }
})();
