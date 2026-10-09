// Drives kdbxweb, KeeWeb's own file layer. kdbxweb and hash-wasm must be resolvable.
// Usage: node keeweb.mjs write <out.kdbx> <password>   writes a fixture Vault file
//        node keeweb.mjs export <in.kdbx> <password>   prints a Vault file as XML
import { readFileSync, writeFileSync } from 'node:fs';
import * as kdbxweb from 'kdbxweb';
import { argon2d, argon2id } from 'hash-wasm';

const [command, path, password] = process.argv.slice(2);

// kdbxweb ships without Argon2; KeeWeb plugs in a WebAssembly build, as we do here.
kdbxweb.CryptoEngine.setArgon2Impl(
  async (pw, salt, memory, iterations, length, parallelism, type, version) => {
    const argon2 = type === kdbxweb.CryptoEngine.Argon2TypeArgon2id ? argon2id : argon2d;
    if (version !== 0x13) throw new Error(`unsupported Argon2 version ${version}`);
    const hash = await argon2({
      password: new Uint8Array(pw),
      salt: new Uint8Array(salt),
      memorySize: memory,
      iterations,
      hashLength: length,
      parallelism,
      outputType: 'binary',
    });
    return hash.buffer;
  },
);

const credentials = new kdbxweb.KdbxCredentials(kdbxweb.ProtectedValue.fromString(password));

if (command === 'export') {
  const file = readFileSync(path);
  const db = await kdbxweb.Kdbx.load(new Uint8Array(file).buffer, credentials);
  // saveXml writes protected attachments as raw bytes, which isn't XML; export them unprotected.
  for (const { hash, value } of db.binaries.getAllWithHashes()) {
    if (value instanceof kdbxweb.ProtectedValue) {
      db.binaries.addWithHash({ hash, value: value.getBinary().buffer });
    }
  }
  process.stdout.write(await db.saveXml(true));
  process.exit(0);
}

const db = kdbxweb.Kdbx.create(credentials, 'KeeWeb fixture');
db.header.dataCipherUuid = new kdbxweb.KdbxUuid(kdbxweb.Consts.CipherId.ChaCha20);
db.setKdf(kdbxweb.Consts.KdfId.Argon2d);
const { ValueType } = kdbxweb.VarDictionary;
db.header.kdfParameters.set('M', ValueType.UInt64, kdbxweb.Int64.from(1024 * 1024));
db.header.kdfParameters.set('I', ValueType.UInt64, kdbxweb.Int64.from(2));
db.header.kdfParameters.set('P', ValueType.UInt32, 1);

// A 1x1 transparent PNG, enough for a custom icon.
const png = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);
const iconId = kdbxweb.KdbxUuid.random();
db.meta.customIcons.set(iconId.id, { data: new Uint8Array(png).buffer, name: 'KeeWeb icon' });
db.meta.customData.set('KeeWebFixture.Settings', { value: 'theme=dark' });

const root = db.getDefaultGroup();
const work = db.createGroup(root, 'Work');

const github = db.createEntry(work);
github.fields.set('Title', 'GitHub');
github.fields.set('UserName', 'octocat');
github.fields.set('Password', kdbxweb.ProtectedValue.fromString('gh-secret-v1'));
github.fields.set('URL', 'https://github.com/login');
github.customIcon = iconId;
github.tags = ['dev', 'work'];
github.fields.set('otp', kdbxweb.ProtectedValue.fromString('otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP'));
github.pushHistory();
github.fields.set('Password', kdbxweb.ProtectedValue.fromString('gh-secret-v2'));
github.times.update();
github.customData = new Map([['KeeWebFixture.EntryState', { value: 'pinned' }]]);

const recovery = await db.createBinary(
  kdbxweb.ProtectedValue.fromBinary(new TextEncoder().encode('keeweb-recovery-code\n').buffer),
);
github.binaries.set('recovery.txt', recovery);

const email = db.createEntry(root);
email.fields.set('Title', 'Email');
email.fields.set('UserName', 'me@example.org');
email.fields.set('Password', kdbxweb.ProtectedValue.fromString('mail-secret'));
email.fields.set('Notes', 'Multi-line\nnotes with <markup> & "quotes"');

writeFileSync(path, Buffer.from(await db.save()));
