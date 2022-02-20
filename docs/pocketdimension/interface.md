# Pocket Dimension Interface

### Types

**Dimension**
The Dimension is simply a `String` value you provide used to group data. Generally you'll provide the module name as the dimension such as `MESSAGING` but realistically you can provide any `String` value.

**DimensionData**
Dimension data is supported in three flavors. A `JSON` object, a `String` value, or a `Buffer`. The cache will recall this type and if possible return the data in the same format you've added it in.

**DimensionQuery**
Dimension queries support the following query types...**TODO**



### Pocket Dimension Interface

`PocketDimension::add_data` Used to add data to **PocketDimension** in relation to the [Modules](modules/overview). 

`PocketDimension::get_data` Used to obtain list of **Data** in relation to the [Modules](modules/overview).

`PocketDimension::size` Returns the total size of **DatA** in relation to the [Modules](modules/overview).

`PocketDimension::count` Returns the count of **Data** in relation to the [Modules](modules/overview) within **PocketDimension**.

`PocketDimension::empty` Flushes out the **Data** related to [Modules](modules/overview)