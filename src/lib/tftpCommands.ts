export type TftpCommandMode = 'download' | 'upload';

export interface BuildTftpCommandOptions {
  mode: TftpCommandMode;
  fileName: string;
  serverIp: string;
  blockSize: number;
}

function quoteShellArgument(value: string): string {
  if (/^[A-Za-z0-9._/+-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

export function buildTftpCommand(options: BuildTftpCommandOptions): string {
  const fileName = quoteShellArgument(options.fileName.trim() || 'firmware.bin');
  const serverIp = quoteShellArgument(options.serverIp.trim() || '<PC_IP>');
  if (options.mode === 'upload') {
    return `tftp -pl ${fileName} ${serverIp}`;
  }
  const blockSize = Number.isFinite(options.blockSize)
    ? Math.min(65464, Math.max(512, Math.trunc(options.blockSize)))
    : 8192;
  return `tftp -gr ${fileName} ${serverIp} -b ${blockSize}`;
}
