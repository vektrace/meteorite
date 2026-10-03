# TODO

- [ ] Registration
- [ ] Login
  - [x] SSO
  - [ ] OAuth
    - [x] Authorization Code
    - [ ] QR Code
  - [x] Username
- [ ] Remove SSO login (will be deprecated, does not need to be removed right now)
- [ ] Device verification (Emoji & QR)
- [ ] Sending & receiving messages
- [ ] Multi-Account
- [ ] VoIP (1:1 & Jitsi)
- [ ] Spaces & Rooms (joining and managing)
- [ ] Threads
- [ ] Account managment
- [ ] GUI
- [ ] GIF Search
- [ ] Write documentation
- [ ] hook up handle_refresh_token function to ui/somewhere else
- [ ] Code-sign application for Apple (IMPORTANT) and Windows
- [ ] Translations (weblate)
- [ ] proper-er error handling for loading screen login
- [ ] return device id on no available refresh token/soft logout (meteorite-core/src/account/auth.rs)
- [ ] possibly add policy/tos uri and purchase new domain???

- TODOs in files:
  - UI
    - [ ] meteorite-ui/src/main.rs: set icon
    - [ ] meteorite-ui/src/main.rs: adjust title based on what the user is doing, e.g. (3) meteorite - Matrix HQ

... and more according to [this](https://spec.matrix.org/v1.19/client-server-api/#summary) (will be added to list later)
