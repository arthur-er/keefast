# Speak the KeePassXC-Browser protocol between extension and app

The Companion extension and the desktop app communicate using the KeePassXC-Browser native-messaging protocol (NaCl-box encrypted messages, associate/pairing keys) instead of a protocol of our own. It has been tested in the field, it already covers pairing, and the stock KeePassXC-Browser extension then works with keefast as a fallback. keefast-only features are added as protocol extensions, never by changing existing messages. Only Chromium-family browsers are targeted for now.
