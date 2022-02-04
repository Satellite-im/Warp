# Warp

Interface Driven Distributed Data Service.

What should be clear if you take nothing else from this documentation... The aim of Warp, is to enable us to run the core tech requirements for Satellite without relying on any specific piece of tech behind the scenes, besides Warp. Meaning we can swap any Web3.0 tech for Web2.0 tech, evolve to WebX.0 and beyond. This should allow our users to chose how they want to run Satellite and give them true freedom to make their own decisions.

#### Overview

Warp is a service that can run as a single binary, providing an interface into the core technologies that run Satellite. This allows us to avoid rewriting the same tech over and over when developing for different platforms. Warp will work on most phones, tablets, computers, and even some consoles. 

It provides abstractions to many different modules which are required to run Satellite. These modules include Messaging, Caching, File Sharing & Storage, RTC connections, and more. Because we focus on building these modules as interfaces first, then allow implementation layers to be built on top of these, it allows us to change the core technologies easily with no extra development required on the "front-end" stacks. This means we can jump from multiple blockchains, or even some other type of decentralized solution without affecting the front end application.

Additionally libraries to interface with Warp (will) exist in JavaScript (TypeScript), Java, Python, and more. So you can easily develop your own platforms and integrations on top of the Satellite tech stack. Lastly a REST API service can be enabled for Warp, however, it should never be exposed outside of `localhost`.