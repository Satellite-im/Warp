# Pocket Dimension (Cacher)

The **PocketDimension** allows [Modules](modules/overview) to store data for quick indexing and searching later on. Additionally, it may be useful to cache frequently used data so that request can be made faster. The **PocketDimension** makes this easy by sorting the data per module, as well as allowing querying by specific information stored inside the payload of the [Data Object](data/overview) for a quick turnaround for search results and other data lookups. Data added to the **PocketDimension** will be versioned using the standard version attached to the [Data Object](data/overview). When data is queried we will always return the latest [Version](data/overview#version) (the greatest number). You can think of this as a giant void to throw data into and query from later.

The **PocketDimension** itself is built on an abstract interface, so we can switch the storage method on a whim. This can include things like Redis, The Graph, FlatFile DBs, etc.

One last thing to note, data stored within a dimension is **never** mutated (only replaced with newer versions), so it can be useful to see how long ago a piece of data was updated by checking the timestamp on that bit of data.


#### Pushing Data

Data can simply be dumped into a dimension, there is not much to worry about other than making sure you're storing the data in the correct dimension. Each module should have at least one of it's own dimensions, it's very rare that two or more modules will share a dimension. 

Updating data is super easy, any time the **PocketDimension** receives an object with an ID that already exists in the dimension, it will version the [Data Object](data/overview) and load it in. 

If we were to create the following data for example:

```js
{
  id: "c59dda79-f2db-4247-b768-b0e093c838d3",
  version: 0,
  timestamp: 1643250387466,
  size: 387192,
  type: {module: "messaging"},
  payload: ...,
}
```

And store it in the dimension providing the `messaging` dimension type, and the [Data Object](data/overview)...

```rust
PocketDimension::add_data(DataType::Module(Module::Messaging), DataObject)
```

It will simply be added to the dimension. If we then update the data we can simply call `PocketDimension::add_data(Module::MESSAGING, ...<DataObject>)` again. Retrieving the data via some query will return the expected object with `version: 1`, `version: 2`, for each iteration of the data in the dimension.


#### Pulling Data

Data can then be retrieved by providing a query using `PocketDimension::get_data`.

```rust
PocketDimension::get_data(DataType::Module(Module::Messaging), None)
```

#### Executing

You **MUST** provide either a query method, or run the `TBD` method to find data. This is to prevent unnecessary 
querying of large amounts of data.

#### Limiting

You can provide a `u16` to the `limit` method to control how many results are returned from the [scan](pocketdimension/overview.md#executing).

**TODO**

#### Filtering

You can filter down the results by [scanning](pocketdimension/overview.md#executing) for specific key values comparators. You'll provide the `key` the filter against, the `value` to expect, and the `modifier`. to filter by.

**TODO**

The `modifier` can be one of the following: **TODO**

#### Query Chaining

You can chain several filters together using the `and` chain.

Example:

**TODO**

#### Example Response

```js
[
    {
      id: "c59dda79-f2db-4247-b768-b0e093c838d3",
      version: 1,
      timestamp: 1643250387466,
      size: 387192,
      type: {module: "messaging"},
      payload: ...,
    },
    {
      id: "c59dda79-f2db-4247-b768-b0e093c838d3",
      version: 2,
      timestamp: 1643250387466,
      size: 387192,
      type: {module: "messaging"},
      payload: ...,
    }
]
```
