import express from "express";
import expressWs from "express-ws";
import pty from "node-pty";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";
import commandLineArgs from "command-line-args";
import commandLineUsage from "command-line-usage";
import { spawn } from "node:child_process";
import net from "node:net";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const argument_option_defs = [
    { name: "help", alias: "h", type: Boolean },
    { name: "port", alias: "p", type: Number, defaultValue: 8080 },
    { name: "command", alias: "c", type: String, defaultValue: "/bin/bash" },
    {
        name: "working-dir",
        alias: "w",
        type: String,
        defaultValue: process.env.HOME,
    },
];
const options = commandLineArgs(argument_option_defs, { stopAtFirstUnknown: true });

if (options.help) {
    const usage = commandLineUsage([
        {
            header: "Arguments",
            optionList: argument_option_defs,
        },
    ]);
    process.exit(0);
}

const SOCKET_PATH = "/tmp/workspace-logger.sock";
const LOGGER_BIN = "/usr/bin/workspace-logger";

const logger = spawn(LOGGER_BIN, ["-s", SOCKET_PATH], {
    stdio: ["ignore", "inherit", "ignore"],
    env: { ...process.env },
});

async function waitForSocket(path, timeoutMs = 3000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        try {
            await new Promise((resolve, reject) => {
                const client = net.createConnection(
                    path.startsWith("@") ? { path: "\0" + path.slice(1) } : { path },
                    () => {
                        client.end();
                        resolve();
                    },
                );
                client.on("error", reject);
            });
            return true;
        } catch {
            await new Promise((r) => setTimeout(r, 100));
        }
    }
    return false;
}

let logSock = null;
if (await waitForSocket(SOCKET_PATH, 3000)) {
    logSock = net.createConnection({ path: SOCKET_PATH, allowHalfOpen: true });
} else {
    console.log("logger not ready; continue without it");
}

function sendCmd(obj) {
    if (!logSock) return;
    logSock.write(JSON.stringify(obj) + "\n");
}

function castFilename(date = new Date()) {
    const pad = (n) => String(n).padStart(2, "0");
    const pad3 = (n) => String(n).padStart(3, "0");

    return (
        [date.getFullYear(), pad(date.getMonth() + 1), pad(date.getDate())].join("-") +
        "_" +
        [pad(date.getHours()), pad(date.getMinutes()), pad(date.getSeconds())].join("-") +
        "." +
        pad3(date.getMilliseconds()) +
        ".cast"
    );
}

const default_env = { LC_CTYPE: "C.UTF-8" };

const app = express();
expressWs(app);

app.use(express.static(path.join(__dirname, "dist")));
app.get("/", (_, res) => {
    res.sendFile(path.join(__dirname, "dist", "index.html"));
});
app.get("/debug", (_, res) => {
    res.sendFile(path.join(__dirname, "dist", "index.html"));
});

let websockets = {};
let ws_id = 0;
let term_output = "";
let term;

const HB_INTERVAL_SEC = (() => {
    const raw = process.env.HB_INTERVAL_SEC;
    const n = Number.parseInt(raw, 10);
    return Number.isFinite(n) && n > 0 ? n : 120;
})();

const CAST_INTERVAL_SEC = (() => {
    const raw = process.env.CAST_INTERVAL_SEC;
    const n = Number.parseInt(raw, 10);
    return Number.isFinite(n) && n > 0 ? n : 120;
})();

let CAST_FULL_PATH = null;

setInterval(() => {
    if (process.env.VERBOSE_LOG && CAST_FULL_PATH) {
        sendCmd({ cmd: "cast_poll", cast: CAST_FULL_PATH });
    }
}, CAST_INTERVAL_SEC * 1000);

function spawn_terminal(ncols = 80, nrows = 24) {
    CAST_FULL_PATH = `/home/student/.local/state/workspace-logs/${castFilename()}`;
    term_output = "";
    term = pty.spawn(options.command, [], {
        name: "xterm-color",
        cols: ncols,
        rows: nrows,
        cwd: options["working-dir"],
        env: { ...default_env, ...process.env, CAST_FULL_PATH: CAST_FULL_PATH },
    });

    term.on("data", (data) => {
        term_output += data;
        for (const ws of Object.values(websockets)) {
            if (ws.readyState === 1) {
                ws.send(data);
            }
        }
    });

    term.on("exit", () => {
        for (const ws of Object.values(websockets)) {
            if (ws.readyState === 1) {
                ws.send("[Process completed]\r\n\r\n");
            }
        }
        if (process.env.VERBOSE_LOG && CAST_FULL_PATH) {
            sendCmd({ cmd: "heartbeat_poll" });
        }
        spawn_terminal(term.cols, term.rows);
    });
}
spawn_terminal();

const LOG_FILE = "/home/student/.local/state/workspace-logs/heartbeat.log";
fs.mkdirSync(path.dirname(LOG_FILE), { recursive: true });
function log_heart_beat(clientId) {
    const now_sec = Math.floor(Date.now() / 1000);
    const line = `${clientId} ${now_sec}\n`;

    fs.appendFile(LOG_FILE, line, (err) => {
        if (err) {
            sendCmd({ cmd: "error", msg: `Failed to append heartbeat log: ${err}` });
        }
    });
}

setInterval(() => {
    if (process.env.VERBOSE_LOG) {
        sendCmd({ cmd: "heartbeat_poll" });
    }
}, HB_INTERVAL_SEC * 1000);

app.ws("/", (ws, _) => {
    const id = ws_id++;
    websockets[id] = ws;
    ws.send(term_output);

    ws.on("message", (msg) => {
        const val = JSON.parse(msg);
        if (val.event === "data") {
            term.write(val.value);
        } else if (val.event === "resize") {
            term.resize(val.value.cols, val.value.rows);
        } else if (val.event === "heartbeat") {
            log_heart_beat(id);
            if (process.env.VERBOSE_LOG) {
                sendCmd({ cmd: "hb", ts: Math.floor(Date.now() / 1e3), session: id });
            }
            ws.send(JSON.stringify({ event: "heartbeat-pong" }));
        }
    });

    ws.on("close", () => {
        delete websockets[id];
    });
});

app.ws("/debug", (ws, _) => {
    const termInstance = pty.spawn("/bin/bash", [], {
        name: "xterm-color",
        cols: 80,
        rows: 24,
        cwd: options["working-dir"],
        env: { ...default_env, ...process.env, WS_DEBUG: "1" },
    });

    let instanceOutput = "";

    termInstance.on("data", (data) => {
        instanceOutput += data;
        if (ws.readyState === 1) {
            ws.send(data);
        }
    });

    ws.send(instanceOutput);

    ws.on("message", (msg) => {
        const val = JSON.parse(msg);
        if (val.event === "data") {
            termInstance.write(val.value);
        } else if (val.event === "resize") {
            termInstance.resize(val.value.cols, val.value.rows);
        } else if (val.event === "heartbeat") {
            ws.send(JSON.stringify({ event: "heartbeat-pong" }));
        }
    });

    ws.on("close", () => {
        termInstance.kill();
    });
});

["SIGINT", "SIGTERM"].forEach((sig) => {
    process.once(sig, () => {
        sendCmd({ cmd: "info", msg: `XTerm server shutting down` });
        if (process.env.VERBOSE_LOG && CAST_FULL_PATH) {
            sendCmd({ cmd: "heartbeat_poll" });
        }
        process.exit(0);
    });
});

app.listen(options.port, () => {
    sendCmd({ cmd: "info", msg: `XTerm server listening at http://localhost:${options.port}` });
});
