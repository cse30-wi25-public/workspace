export const mapColemak = {
    KeyQ: "q",
    KeyW: "w",
    KeyE: "f",
    KeyR: "p",
    KeyT: "g",
    KeyY: "j",
    KeyU: "l",
    KeyI: "u",
    KeyO: "y",
    KeyP: ";",
    KeyA: "a",
    KeyS: "r",
    KeyD: "s",
    KeyF: "t",
    KeyG: "d",
    KeyH: "h",
    KeyJ: "n",
    KeyK: "e",
    KeyL: "i",
    Semicolon: "o",
    KeyZ: "z",
    KeyX: "x",
    KeyC: "c",
    KeyV: "v",
    KeyB: "b",
    KeyN: "k",
    KeyM: "m",
};
const layouts = { qwerty: null, colemak: mapColemak };

const shifted = { ";": ":", ":": ":" };

export function makeKeyHandler(socket, getLayout) {
    let swallowNextKeypress = false;

    return function handleKey(ev) {
        if (ev.type === "keypress") {
            if (swallowNextKeypress) {
                swallowNextKeypress = false;
                return false;
            }
            return true;
        }
        if (ev.type !== "keydown" || ev.repeat) return true;

        const table = layouts[getLayout()] ?? null;
        const base = table ? table[ev.code] : null;
        if (!base) return true;

        socket.send(JSON.stringify({ event: "data", value: buildSeq(ev, base) }));
        swallowNextKeypress = true;
        return false;
    };
}

function buildSeq(e, base) {
    const caps = e.getModifierState("CapsLock");
    const wantUpper = caps ? !e.shiftKey : e.shiftKey;
    const upper = base.toUpperCase();
    const lower = base.toLowerCase();
    const withShift = shifted[upper] ?? upper;

    if (e.ctrlKey && (e.altKey || e.metaKey)) return "\x1B" + ctrlCode(upper);

    if (e.ctrlKey) return ctrlCode(upper);

    if (e.altKey || e.metaKey) return "\x1B" + (wantUpper ? withShift : lower);

    return wantUpper ? withShift : lower;
}

const ctrlCode = (ch) => String.fromCharCode(ch.charCodeAt(0) & 0x1f);
