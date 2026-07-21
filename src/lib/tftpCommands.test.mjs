import assert from 'node:assert/strict';

import { buildTftpCommand } from './tftpCommands.ts';

assert.equal(
  buildTftpCommand({
    mode: 'download',
    fileName: 'tcpdump',
    serverIp: '192.168.3.11',
    blockSize: 8192,
  }),
  'tftp -gr tcpdump 192.168.3.11 -b 8192',
);

assert.equal(
  buildTftpCommand({
    mode: 'upload',
    fileName: 'tcpdump',
    serverIp: '192.168.3.11',
    blockSize: 8192,
  }),
  'tftp -pl tcpdump 192.168.3.11',
);

assert.equal(
  buildTftpCommand({
    mode: 'download',
    fileName: 'release files/image.bin',
    serverIp: '192.168.4.96',
    blockSize: 8192,
  }),
  "tftp -gr 'release files/image.bin' 192.168.4.96 -b 8192",
);

console.log('tftpCommands tests PASSED');
