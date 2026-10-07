// Drives KeePassLib, KeePass2's own file layer.
// Usage: keepass2.exe write <out.kdbx> <password> kdbx3|kdbx4   writes a fixture Vault file
//        keepass2.exe export <in.kdbx> <password>              prints a Vault file as XML
using System;
using System.Reflection;
using System.Text;
using KeePassLib;
using KeePassLib.Cryptography.Cipher;
using KeePassLib.Cryptography.KeyDerivation;
using KeePassLib.Interfaces;
using KeePassLib.Keys;
using KeePassLib.Security;
using KeePassLib.Serialization;

static class Fixture
{
    // A 1x1 transparent PNG, enough for a custom icon.
    const string IconPng =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    static PwEntry AddEntry(PwGroup group, string title, string user, string password, string url)
    {
        var entry = new PwEntry(true, true);
        entry.Strings.Set(PwDefs.TitleField, new ProtectedString(false, title));
        entry.Strings.Set(PwDefs.UserNameField, new ProtectedString(false, user));
        entry.Strings.Set(PwDefs.PasswordField, new ProtectedString(true, password));
        entry.Strings.Set(PwDefs.UrlField, new ProtectedString(false, url));
        group.AddEntry(entry, true);
        return entry;
    }

    static CompositeKey Key(string password)
    {
        var key = new CompositeKey();
        key.AddUserKey(new KcpPassword(password));
        return key;
    }

    static void Main(string[] args)
    {
        if (args[0] == "export") Export(args[1], args[2]);
        else Write(args[1], args[2], args[3] == "kdbx4");
    }

    static void Export(string path, string password)
    {
        var db = new PwDatabase();
        db.Open(IOConnectionInfo.FromPath(path), Key(password), new NullStatusLogger());
        using (var stdout = Console.OpenStandardOutput())
            new KdbxFile(db).Save(stdout, null, KdbxFormat.PlainXml, new NullStatusLogger());
    }

    static void Write(string path, string password, bool kdbx4)
    {
        var key = Key(password);
        var db = new PwDatabase();
        db.New(new IOConnectionInfo(), key);
        db.Name = "KeePass2 fixture";

        if (kdbx4)
        {
            db.DataCipherUuid = new ChaCha20Engine().CipherUuid;
            var kdf = new Argon2Kdf(Argon2Type.ID);
            var p = kdf.GetDefaultParameters();
            p.SetUInt64(Argon2Kdf.ParamMemory, 1024 * 1024);
            p.SetUInt64(Argon2Kdf.ParamIterations, 2);
            p.SetUInt32(Argon2Kdf.ParamParallelism, 1);
            db.KdfParameters = p;
        }
        else
        {
            db.DataCipherUuid = new StandardAesEngine().CipherUuid;
            var p = new AesKdf().GetDefaultParameters();
            p.SetUInt64(AesKdf.ParamRounds, 1000);
            db.KdfParameters = p;
        }

        // Plugin data lives in the Vault's custom data, which KDBX 3.1 supports too.
        db.CustomData.Set("KeePassFixturePlugin.Settings", "{\"enabled\":true}");

        var icon = new PwCustomIcon(new PwUuid(true), Convert.FromBase64String(IconPng));
        // Named icons are a KDBX 4.1 feature; KeePassLib would upgrade the file for them.
        if (kdbx4) icon.Name = "Fixture icon";
        db.CustomIcons.Add(icon);

        var work = new PwGroup(true, true, "Work", PwIcon.Folder);
        db.RootGroup.AddGroup(work, true);
        var personal = new PwGroup(true, true, "Personal", PwIcon.Folder);
        db.RootGroup.AddGroup(personal, true);

        var github = AddEntry(work, "GitHub", "octocat", "gh-secret-v1", "https://github.com/login");
        github.CustomIconUuid = icon.Uuid;
        github.Tags.Add("dev");
        github.Strings.Set("Recovery question", new ProtectedString(true, "first-pet"));

        // History: keep the first password, then change it.
        github.CreateBackup(db);
        github.Strings.Set(PwDefs.PasswordField, new ProtectedString(true, "gh-secret-v2"));
        github.Touch(true);

        var recovery = Encoding.UTF8.GetBytes("recovery-code-1\nrecovery-code-2\n");
        github.Binaries.Set("recovery-codes.txt", new ProtectedBinary(false, recovery));
        var email = AddEntry(personal, "Email", "me@example.org", "mail-secret", "https://mail.example.org");
        // The same attachment on two Entries is stored once in the binary pool.
        email.Binaries.Set("recovery-codes.txt", new ProtectedBinary(false, recovery));
        email.Binaries.Set("protected.bin", new ProtectedBinary(true, new byte[] { 0, 1, 2, 3, 254, 255 }));

        var gone = AddEntry(personal, "Deleted", "nobody", "x", "");
        personal.Entries.Remove(gone);
        db.DeletedObjects.Add(new PwDeletedObject(gone.Uuid, DateTime.UtcNow));

        if (kdbx4)
        {
            // Entry and Group custom data need KDBX 4.
            github.CustomData.Set("KeePassFixturePlugin.EntryState", "synced");
            work.CustomData.Set("KeePassFixturePlugin.GroupState", "expanded");
        }

        // KeePass 2.61 never picks KDBX 3.1 by itself; its "KeePass KDBX (2.34)" export forces it
        // through this internal property.
        var file = new KdbxFile(db);
        if (!kdbx4)
            typeof(KdbxFile)
                .GetProperty("ForceVersion", BindingFlags.Instance | BindingFlags.NonPublic)
                .SetValue(file, 0x00030001u, null);
        using (var stream = System.IO.File.Create(path))
            file.Save(stream, null, KdbxFormat.Default, new NullStatusLogger());
    }
}
