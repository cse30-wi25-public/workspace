import { Terminal } from '@xterm/xterm';
import { ClipboardAddon } from '@xterm/addon-clipboard';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

function initTerminal() {
    const term = new Terminal({
        fontFamily: 'courier new, courier, monospace',
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    const clipboardAddon = new ClipboardAddon();
    term.loadAddon(clipboardAddon);

    const container = document.getElementById('terminal');
    term.open(container);
    fitAddon.fit();

    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsPath = location.pathname;
    const socket = new WebSocket(`${protocol}//${location.host}${wsPath}`);

    socket.onopen = () => {
        term.onData((data) => {
            socket.send(JSON.stringify({ event: 'data', value: data }));
        });
        term.attachCustomKeyEventHandler((e) => {
            if (e.ctrlKey && e.code === 'KeyP') {
                console.log("!!!");
                const selection = term.getSelection();
                if (selection) {
                    navigator.clipboard.writeText(selection)
                        .then(() => {
                            console.log('Copied to clipboard:', selection);
                        })
                        .catch(err => {
                            console.error('Error copying to clipboard:', err);
                        });
                }
                return false;
            }

            return true;
        });


        function doResize() {
            fitAddon.fit();
            socket.send(
                JSON.stringify({
                    event: 'resize',
                    value: { rows: term.rows, cols: term.cols },
                })
            );
        }
        window.addEventListener('resize', doResize);
        doResize();

        setInterval(() => {
            socket.send(JSON.stringify({ event: 'heartbeat' }));
        }, 10_000);

        socket.onmessage = (msg) => {
            let dataObj;
            try {
                dataObj = JSON.parse(msg.data);
            } catch (err) {
                term.write(msg.data);
                return;
            }
            if (dataObj.event === 'heartbeat-pong') {
                console.log('[Client] heartbeat-pong');
            } else {
                term.write(msg.data);
            }
        };
    };
}

document.addEventListener('DOMContentLoaded', initTerminal);

