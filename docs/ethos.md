# Technical Ethos

Building on Warp should remain easy for cross platorm compatibility, in addition extending the platform should remain simple. The underlying tech stack should also never be oppinionated towards one technical solution to meet the demand for a decentralized P2P architecture. Developers should be able to use anything from Web3.0 to Web2.0 if they so choose. 

Warp was built with the idea of packing all of the core functionality required to build an application like Satellite into one easily deployable binary. The binary will handle peer connections, interfacing with blockchains, as well as storing sensitive user-information in a secure encrypted environment. 

Extensions built on top of Warp should override the common interface for that module as well as extending it. We should be sure that the core I/O of the extensions matches the core interface so that there is no additional legwork needed for common functionality on the UI.

Lastly, build with the idea that there will be others looking at your code in the future and you should be able to easily modify said code in the future. We are building for the open source community and welcome others to come contribute to the project!