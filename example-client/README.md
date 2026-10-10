# Welcome from the client

The addon prints one line when it loads. The UI Transform adds a second welcome to the default
chat frame on world entry. It inserts code after `ChatFrame_OnEvent` in the client's own
`Interface/FrameXML/ChatFrame.lua`.

Install with `./lyracore packages add example-client`, run `./lyracore client sync`, then restart
the client. This Package needs no Module change. The UI Transform requires a 1.12.1 Baseline with
one matching anchor. It ships only the insertion, never the Baseline. `client pack` refuses a
distributable Client Pack that contains a UI Transform's derived output.
