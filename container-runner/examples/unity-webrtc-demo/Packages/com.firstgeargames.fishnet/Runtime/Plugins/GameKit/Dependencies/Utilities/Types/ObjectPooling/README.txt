IMPORTANT: this project requires the following packages.
- Fundamentals.


Fast Object Pool
----------------

Setup:
	- Add ObjectPool to any object in your scene.
	- (Optional) Add ObjectPool under a DontDestroyOnLoad object for it to persist through scene changes.

Usage:
	- Return objects by using ObjectPool.Retrieve().
	- You may return types, such as scripts, by using ObjectPool.Retrieve<YourScript>().
	- Send objects back to the pool using ObjectPool.Store(). You may also store using a delay, like as with Destroy(obj, delay).

Notes:
	- All of Fast Object Pool Retrieve() methods mimic Unity Instantiate() overrides.
