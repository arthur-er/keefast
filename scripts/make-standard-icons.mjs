// Regenerates crates/app/assets/icons/standard/: the pictures keefast draws for KeePass's 69
// standard icons, taken from Lucide (ISC, see the LICENSE there). KeePass numbers its standard
// icons; each client draws them its own way, so this picks the Lucide icon nearest in meaning.
//
// Usage: node scripts/make-standard-icons.mjs <path to an installed lucide-react package>

import { mkdirSync, readFileSync, writeFileSync, copyFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// KeePass's names (PwIcon), in order, with the Lucide icon drawn for each.
const ICONS = [
  ['Key', 'key'],
  ['World', 'globe'],
  ['Warning', 'triangle-alert'],
  ['NetworkServer', 'server'],
  ['MarkedDirectory', 'folder-check'],
  ['UserCommunication', 'messages-square'],
  ['Parts', 'puzzle'],
  ['Notepad', 'notepad-text'],
  ['WorldSocket', 'plug'],
  ['Identity', 'id-card'],
  ['PaperReady', 'file-check'],
  ['Digicam', 'camera'],
  ['IRCommunication', 'radio'],
  ['MultiKeys', 'key-round'],
  ['Energy', 'zap'],
  ['Scanner', 'scan'],
  ['WorldStar', 'sparkles'],
  ['CDRom', 'disc'],
  ['Monitor', 'monitor'],
  ['EMail', 'mail'],
  ['Configuration', 'settings'],
  ['ClipboardReady', 'clipboard-check'],
  ['PaperNew', 'file-plus'],
  ['Screen', 'tv'],
  ['EnergyCareful', 'battery-warning'],
  ['EMailBox', 'mailbox'],
  ['Disk', 'save'],
  ['Drive', 'hard-drive'],
  ['PaperQ', 'file-question-mark'],
  ['TerminalEncrypted', 'square-terminal'],
  ['Console', 'terminal'],
  ['Printer', 'printer'],
  ['ProgramIcons', 'layout-grid'],
  ['Run', 'play'],
  ['Settings', 'wrench'],
  ['WorldComputer', 'network'],
  ['Archive', 'archive'],
  ['Homebanking', 'landmark'],
  ['DriveWindows', 'app-window'],
  ['Clock', 'clock'],
  ['EMailSearch', 'mail-search'],
  ['PaperFlag', 'flag'],
  ['Memory', 'memory-stick'],
  ['TrashBin', 'trash-2'],
  ['Note', 'sticky-note'],
  ['Expired', 'ban'],
  ['Info', 'info'],
  ['Package', 'package'],
  ['Folder', 'folder'],
  ['FolderOpen', 'folder-open'],
  ['FolderPackage', 'folder-archive'],
  ['LockOpen', 'lock-open'],
  ['PaperLocked', 'file-lock'],
  ['Checked', 'circle-check'],
  ['Pen', 'pen'],
  ['Thumbnail', 'image'],
  ['Book', 'book'],
  ['List', 'list'],
  ['UserKey', 'user'],
  ['Tool', 'hammer'],
  ['Home', 'house'],
  ['Star', 'star'],
  ['Tux', 'bird'],
  ['Feather', 'feather'],
  ['Apple', 'apple'],
  ['Wiki', 'book-open'],
  ['Money', 'banknote'],
  ['Certificate', 'award'],
  ['BlackBerry', 'smartphone'],
];

const lucide = process.argv[2];
if (!lucide) {
  console.error('usage: node scripts/make-standard-icons.mjs <lucide-react package directory>');
  process.exit(1);
}
const out = join(dirname(fileURLToPath(import.meta.url)), '../crates/app/assets/icons/standard');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
copyFileSync(join(lucide, 'LICENSE'), join(out, 'LICENSE'));

const attributes = (props) =>
  Object.entries(props)
    .filter(([name]) => name !== 'key')
    .map(([name, value]) => `${name}="${value}"`)
    .join(' ');

for (const [index, [keepass, name]] of ICONS.entries()) {
  const module = await import(pathToFileURL(join(lucide, 'dist/esm/icons', `${name}.mjs`)));
  const shapes = module.__iconNode.map(([tag, props]) => `<${tag} ${attributes(props)}/>`);
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" ' +
    'fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" ' +
    `stroke-linejoin="round"><!-- ${keepass}: Lucide ${name} -->${shapes.join('')}</svg>\n`;
  writeFileSync(join(out, `${String(index).padStart(2, '0')}.svg`), svg);
}
console.log(`wrote ${ICONS.length} icons to ${out}`);
