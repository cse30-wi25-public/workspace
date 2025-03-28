import express from 'express';
import expressWs from 'express-ws';
import pty from 'node-pty';
import path from 'path';
import { fileURLToPath } from 'url';
import commandLineArgs from 'command-line-args';
import commandLineUsage from 'command-line-usage';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const argument_option_defs = [
    { name: 'help', alias: 'h', type: Boolean },
    { name: 'port', alias: 'p', type: Number, defaultValue: 8080 },
    { name: 'command', alias: 'c', type: String, defaultValue: '/bin/bash' },
    {
        name: 'working-dir',
        alias: 'w',
        type: String,
        defaultValue: process.env.HOME,
    },
];
const options = commandLineArgs(argument_option_defs);
if (options.help) {
    const usage = commandLineUsage([
        {
            header: 'Arguments',
            optionList: argument_option_defs,
        },
    ]);
    console.log(usage);
    process.exit(0);
}

const default_env = { LC_CTYPE: 'C.UTF-8' };

const app = express();
expressWs(app);

app.use(express.static(path.join(__dirname, 'dist')));
app.get('/', (req, res) => {
  res.sendFile(path.join(__dirname, 'dist', 'index.html'));
});
app.get('/debug', (req, res) => {
  res.sendFile(path.join(__dirname, 'dist', 'index.html'));
});

let websockets = {};
let ws_id = 0;
let term_output = '';
let term;

function spawn_terminal() {
    term_output = '';
    term = pty.spawn(options.command, [], {
        name: 'xterm-color',
        cols: 80,
        rows: 24,
        cwd: options['working-dir'],
        env: { ...default_env, ...process.env },
    });

    term.on('data', (data) => {
        term_output += data;
        for (const ws of Object.values(websockets)) {
            if (ws.readyState === 1) {
                ws.send(data);
            }
        }
    });

    term.on('exit', () => {
        for (const ws of Object.values(websockets)) {
            if (ws.readyState === 1) {
                ws.send('[Process completed]\r\n\r\n');
            }
        }
        spawn_terminal();
    });
}
spawn_terminal();

app.ws('/', (ws, req) => {
    const id = ws_id++;
    websockets[id] = ws;
    ws.send(term_output);

    ws.on('message', (msg) => {
        const val = JSON.parse(msg);
        if (val.event === 'data') {
            term.write(val.value);
        } else if (val.event === 'resize') {
            term.resize(val.value.cols, val.value.rows);
        } else if (val.event === 'heartbeat') {
            ws.send(JSON.stringify({ event: 'heartbeat-pong' }));
        }
    });

    ws.on('close', () => {
        delete websockets[id];
    });
});

app.ws('/debug', (ws, req) => {
    const termInstance = pty.spawn(options.command, [], {
        name: 'xterm-color',
        cols: 80,
        rows: 24,
        cwd: options['working-dir'],
        env: { ...default_env, ...process.env, WS_DEBUG: '1' },
    });

    let instanceOutput = '';

    termInstance.on('data', (data) => {
        instanceOutput += data;
        if (ws.readyState === 1) {
            ws.send(data);
        }
    });

    ws.send(instanceOutput);

    ws.on('message', (msg) => {
        const val = JSON.parse(msg);
        if (val.event === 'data') {
            termInstance.write(val.value);
        } else if (val.event === 'resize') {
            termInstance.resize(val.value.cols, val.value.rows);
        } else if (val.event === 'heartbeat') {
            ws.send(JSON.stringify({ event: 'heartbeat-pong' }));
        }
    });

    ws.on('close', () => {
        termInstance.kill();
    });
});

app.listen(options.port, () => {
    console.log(`XTerm server listening at http://localhost:${options.port}`);
});

