# Pocket Dimension Interface

#### Types

**Dimension**
The Dimension is simply a `String` value you provide used to group data. Generally you'll provide the module name as the dimension such as `MESSAGING` but realistically you can provide any `String` value.

**DimensionData**
Dimension data is supported in three flavors. A `JSON` object, a `String` value, or a `Buffer`. The cache will recall this type and if possible return the data in the same format you've added it in.

**DimensionQuery**
Dimension queries support the following query types...**TODO**



#### Input Data

Description

```rust
add(dimension: Module, data: Data)
```

```rust
get(dimension: Module)
```

```rust
where(modifier: QueryModifier)
// or
and(modifier: QueryModifier)
```


```rust
size(dimension: Module)
```

```rust
count(dimension: Module)
```

```rust
empty(dimension: Module)
```