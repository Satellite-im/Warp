# Data Standard

## Overview

Warp sends and receives all data in a standardized "Data Object". **Data Objects** which are not formatted in the 
correct way will be rejected by the service. This allows a very predictable way to send **Data Objects** in and out 
of the service while keeping the specific implementations of each module agnostic. We can expect the same type of 
**Data Object** to come out of, for instance, the [Constellation](constellation/overview.md) whether we're storing 
data on 
disk, using IPFS, WebTorrent, or any other implementation. You should reference each module for specific [Payload](#payload) type. Each module you call upon should also provide its type, be it through an interface module in 
TypeScript, a JSON object from the API, or some other method for your specific programming language. The **Data Object** should always be a precursor to any module specific data, even if the data is coming straight from the module itself.

Lastly, to maintain data types, Warp uses the [Serde](https://serde.rs/) to serialize data.

## Usage

### Data Structure

**See also:** [Module](modules/interface.md), [Payload](data/standard.md#Payload)

#### ID

A unique identifier useful for distinguish similar but different data coming in and out of the Warp service.

#### Version

The version is managed automatically inside of Warp. This is used to update data in the **Pocket Dimension**.

#### Timestamp

This is simply an **EPOCH** timestamp which denotes the exact moment the **Data Object** was created.

#### Size

Each **Data Object** will contain the size of the total data payload in **bytes**. This can be useful for tracking local network throughput or resource usage of 
each module and the way you're using it.

#### DataType

The data type represents the associated payload. Each type is required to have a unique [DataType](data/data_types) identifier attached 
to the data. You can get a list of the available module types by checking the API.

#### Payload

**Each module will format it's payload in it's own way.** This makes the [Module](modules/core_types) critical for parsing the data object that comes from the Warp service. You should refer to each module to figure out how data is formatted.

