# Plugins

These are implementation using warp modules. Some plugins written are examples to show how it can be used. 

## warp-pd-stretto

Pocket Dimension implementation with [Stretto](https://github.com/al8n/stretto), a high performance thread-safe memory 
cache written in rust. This allows for in-memory caching and should be used for development purpose.

## warp-pd-flatfile

Flatfile implementation for Pocket Dimension to cache data to disk. 

## warp-fs-memory

In-memory filesystem implementation. **This extension is designed for development purpose only and should not be used in production**

## warp-fs-storj

Implementation of [StorJ](https://storj.io) for Constellation, a decentralized cloud storage provider. This extension utilizes StorJ S3 compatible API. 

### Note

This extension will require access and secret keys from StorJ to operate. You can get them by [signing up](https://us1.storj.io/signup). Go to []() for more information

## warp-fs-ipfs

Implementation of [IPFS](https://ipfs.io/) for Constellation. a peer-to-peer protocol for storing and accessing various of data. This extension connects to an IPFS node via HTTP API. 

### Note

For this extension to work, one would need to have a IPFS node installed or connect to a IPFS node via HTTP.

## warp-mp-solana

**TODO**

## warp-rg-ipfs

**TODO**