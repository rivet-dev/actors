using GameKit.Dependencies.Utilities;
using System.Collections;
using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.ObjectPooling
{
    public class ObjectPool : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// Data for a delayed store object.
        /// </summary>
        private struct DelayedStoreData
        {
            public DelayedStoreData(float storeTime, bool parentPooler)
            {
                StoreTime = storeTime;
                ParentPooler = parentPooler;
            }

            public readonly float StoreTime;
            public readonly bool ParentPooler;
        }
        #endregion

        #region Serialized.
        /// <summary>
        /// Time to wait before destroying object pools with no activity as well pool entries which haven't been used recently. Use -1f to disable this feature.
        /// </summary>
        [Tooltip("Time to wait before destroying object pools with no activity as well pool entries which haven't been used recently. Use -1f to disable this feature.")]
        [SerializeField]
        private float _dataExpirationDelay = 60f;
        #endregion

        #region Private.
        /// <summary>
        /// A singleton instance of this script.
        /// </summary>
        private static ObjectPool _instance;
        /// <summary>
        /// Transform which houses all pooled objects.
        /// </summary>
        private Transform _collector = null;
        /// <summary>
        /// List of all object pools.
        /// </summary>
        private List<PoolData> _pools = new();
        /// <summary>
        /// Holds transform information for categories of pooled objects. Used to sort pooled objects for easier hierarchy navigation.
        /// </summary>
        private Dictionary<string, Transform> _categoryChildren = new();
        /// <summary>
        /// Stores which PoolData prefabs are using.
        /// </summary>
        private Dictionary<GameObject, PoolData> _poolPrefabs = new();
        /// <summary>
        /// Stores which Pooldata retrieved objects belong to.
        /// </summary>
        private Dictionary<GameObject, PoolData> _activeObjects = new();
        /// <summary>
        /// Objects which will be stored after a delay.
        /// </summary>
        private Dictionary<GameObject, DelayedStoreData> _delayedStoreObjects = new();
        #endregion

        private void Awake()
        {
            // Make sure there is only once instance.
            if (_instance != null && _instance != this)
            {
                if (Debug.isDebugBuild)
                    Debug.LogWarning("Multiple ObjectPool scripts found. This script auto loads itself and does not need to be placed in your scenes.");
                Destroy(this);
                return;
            }
            else
            {
                _instance = this;
            }
        }

        private void Update() { }

        private void Start()
        {
            StartCoroutine(__CleanupChecks());
        }

        /// <summary>
        /// Destroys null objects within SpawnedObjects.
        /// </summary>
        private IEnumerator __CleanupChecks()
        {
            int poolIndex = 0;

            while (true)
            {
                /* Check which objects must be delayed stored first.
                 * This is so objects which need to be stored will
                 * be added to their appropriate pool before it
                 * potentially gets expired, which would double the work
                 * because then the pool would get recreated. */
                if (_delayedStoreObjects.Count > 0)
                {
                    List<GameObject> keysToRemove = new();
                    foreach (KeyValuePair<GameObject, DelayedStoreData> item in _delayedStoreObjects)
                    {
                        if (Time.time >= item.Value.StoreTime)
                        {
                            //Add to collection of keys to remove after.
                            keysToRemove.Add(item.Key);
                            //Store object.
                            Store(item.Key, item.Value.ParentPooler);
                        }
                    }
                    //Now store away keys to remove, and get them out of the dictionary.
                    for (int i = 0; i < keysToRemove.Count; i++)
                        _delayedStoreObjects.Remove(keysToRemove[i]);
                }

                if (_dataExpirationDelay > 0f && _pools.Count > 0)
                {
                    if (poolIndex >= _pools.Count)
                        poolIndex = 0;

                    //If pool hasn't been used in awhile then expire it.
                    if (_pools[poolIndex].PoolExpired())
                    {
                        _poolPrefabs.Remove(_pools[poolIndex].Prefab);
                        DestroyPool(_pools[poolIndex], false);
                        _pools.RemoveAt(poolIndex);
                        poolIndex--;
                    }
                    //Not expired. Try to cull the pool.
                    else
                    {
                        List<GameObject> culledObjects = _pools[poolIndex].Cull();
                        for (int i = 0; i < culledObjects.Count; i++)
                        {
                            Destroy(culledObjects[i]);
                        }
                    }

                    poolIndex++;
                }

                //Wait a frame.
                yield return null;
            }
        }

        /// <summary>
        /// Destroys all stored and optionally retrieved objects. Race conditions may be created by this coroutine if trying to retrieve or store objects before it finishes executing.
        /// </summary>
        /// <param name = "destroyActive">True to also destroy active retrieved gameObjects. False will erase active objects from memory, but not destroy them.</param>
        public IEnumerator __Reset(bool destroyActive)
        {
            /* Clear references to which pool is for each prefab,
             * as well to which transform spawned objects child to when stored. */
            _poolPrefabs.Clear();
            _categoryChildren.Clear();
            _pools.Clear();
            /* When resetting clear the delayed store
             * objects. If destroyActive is set to true
             * then they will be picked up. */
            _delayedStoreObjects.Clear();

            //Destory all children.
            transform.DestroyChildren(false);

            WaitForEndOfFrame endOfFrame = new();
            while (transform.childCount > 0)
                yield return endOfFrame;

            //Destroy active objects.
            if (destroyActive)
            {
                //Make a new list to prevent collection from being modified.
                List<GameObject> objects = new();
                foreach (KeyValuePair<GameObject, PoolData> value in _activeObjects)
                    objects.Add(value.Key);
                //Go through collection.
                for (int i = 0; i < objects.Count; i++)
                {
                    if (objects[i] != null)
                    {
                        Destroy(objects[i]);
                        while (objects[i] != null)
                            yield return endOfFrame;
                    }
                }
            }
            _activeObjects.Clear();
        }

        /// <summary>
        /// Destroys all Objects within the specified PoolData then clears the PoolData.
        /// </summary>
        /// <param name = "poolData"></param>
        /// <param name = "removeFromList">True to remove from pools list.</param>
        private void DestroyPool(PoolData poolData, bool removeFromList)
        {
            for (int i = 0; i < poolData.Objects.Entries.Count; i++)
            {
                if (poolData.Objects.Entries[i] != null)
                    Destroy(poolData.Objects.Entries[i]);
            }

            if (removeFromList)
                _pools.Remove(poolData);
        }

        /// <summary>
        /// Returns the PoolData which houses the desired prefab.
        /// </summary>
        /// <param name = "prefab">Prefab to return a pool for.</param>
        /// <returns></returns>
        private PoolData ReturnPoolData(GameObject prefab)
        {
            PoolData result;
            _poolPrefabs.TryGetValue(prefab, out result);

            if (result == null)
                result = CreatePool(prefab);

            return result;
        }

        /// <summary>
        /// Sets the position and rotation of the specified GameObject.
        /// </summary>
        /// <param name = "result"></param>
        /// <param name = "position"></param>
        /// <param name = "rotation"></param>
        private void SetGameObjectPositionRotation(GameObject result, Vector3 position, Quaternion rotation)
        {
            result.transform.position = position;
            result.transform.rotation = rotation;
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "poolObject">Prefab to retrieve.</param>
        /// <returns></returns>
        public static GameObject Retrieve(GameObject poolObject)
        {
            return _instance.RetrieveInternal(poolObject);
        }

        private GameObject RetrieveInternal(GameObject poolObject)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(poolObject, out pool);

            //If pool isn't null then reset result.
            if (result != null && pool != null)
            {
                SetGameObjectPositionRotation(result, pool.Prefab.transform.position, pool.Prefab.transform.rotation);
                result.transform.SetParent(null);
            }

            return FinalizeRetrieve(result, pool);
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "poolObject">Prefab to retrieve.</param>
        /// <param name = "parent">Parent to attach the retrieved prefab to.</param>
        /// <param name = "instantiateInWorldSpace">Use true when assigning a parent Object to maintain the world position of the Object, instead of setting its position relative to the new parent. Pass false to set the Object's position relative to its new parent.</param>
        /// <returns></returns>
        public static GameObject Retrieve(GameObject poolObject, Transform parent, bool instantiateInWorldSpace = true)
        {
            return _instance.RetrieveInternal(poolObject, parent, instantiateInWorldSpace);
        }

        public GameObject RetrieveInternal(GameObject poolObject, Transform parent, bool instantiateInWorldSpace = true)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(poolObject, out pool);

            //If pool isn't null then reset result.
            if (result != null && pool != null)
            {
                SetGameObjectPositionRotation(result, pool.Prefab.transform.position, pool.Prefab.transform.rotation);
                result.transform.SetParent(parent, instantiateInWorldSpace);
            }

            return FinalizeRetrieve(result, pool);
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "poolObject">Prefab to retrieve.</param>
        /// <param name = "position">Position for the retrieved object.</param>
        /// <param name = "rotation">Orientation for the retrieved object.</param>
        /// <returns></returns>
        public static GameObject Retrieve(GameObject poolObject, Vector3 position, Quaternion rotation)
        {
            return _instance.RetrieveInternal(poolObject, position, rotation);
        }

        private GameObject RetrieveInternal(GameObject poolObject, Vector3 position, Quaternion rotation)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(poolObject, out pool);

            //If pool isn't null then reset result.
            if (result != null)
            {
                SetGameObjectPositionRotation(result, position, rotation);
                result.transform.SetParent(null);
            }

            return FinalizeRetrieve(result, pool);
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "poolObject">Prefab to retrieve.</param>
        /// <param name = "position">Position for the retrieved object.</param>
        /// <param name = "rotation">Orientation for the retrieved object.</param>
        /// <param name = "parent">Transform to parent the retrieved object to.</param>
        /// <returns></returns>
        public GameObject Retrieve(GameObject poolObject, Vector3 position, Quaternion rotation, Transform parent)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(poolObject, out pool);

            //If pool isn't null then reset result.
            if (result != null)
            {
                SetGameObjectPositionRotation(result, position, rotation);
                result.transform.SetParent(parent, true);
            }

            return FinalizeRetrieve(result, pool);
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "prefab">Prefab to retrieve.</param>
        /// <returns></returns>
        public static T Retrieve<T>(GameObject prefab)
        {
            return _instance.RetrieveInternal<T>(prefab);
        }

        private T RetrieveInternal<T>(GameObject prefab)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(prefab, out pool);

            //If pool isn't null then reset result.
            if (result != null && pool != null)
            {
                SetGameObjectPositionRotation(result, pool.Prefab.transform.position, pool.Prefab.transform.rotation);
                result.transform.SetParent(null);
            }

            return FinalizeRetrieve(result, pool).GetComponent<T>();
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "prefab">Prefab to retrieve.</param>
        /// <param name = "parent">Parent to attach the retrieved prefab to.</param>
        /// <param name = "instantiateInWorldSpace">Use true when assigning a parent Object to maintain the world position of the Object, instead of setting its position relative to the new parent. Pass false to set the Object's position relative to its new parent.</param>
        /// <returns></returns>
        public static T Retrieve<T>(GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
        {
            return _instance.RetrieveInternal<T>(prefab, parent, instantiateInWorldSpace);
        }

        private T RetrieveInternal<T>(GameObject prefab, Transform parent, bool instantiateInWorldSpace = true)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(prefab, out pool);

            //If pool isn't null then reset result.
            if (result != null && pool != null)
            {
                SetGameObjectPositionRotation(result, pool.Prefab.transform.position, pool.Prefab.transform.rotation);
                result.transform.SetParent(parent, instantiateInWorldSpace);
            }

            return FinalizeRetrieve(result, pool).GetComponent<T>();
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "prefab">Prefab to retrieve.</param>
        /// <param name = "position">Position for the retrieved object.</param>
        /// <param name = "rotation">Orientation for the retrieved object.</param>
        /// <returns></returns>
        public static T Retrieve<T>(GameObject prefab, Vector3 position, Quaternion rotation)
        {
            return _instance.RetrieveInternal<T>(prefab, position, rotation);
        }

        private T RetrieveInternal<T>(GameObject prefab, Vector3 position, Quaternion rotation)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(prefab, out pool);

            //If pool isn't null then reset result.
            if (result != null)
            {
                SetGameObjectPositionRotation(result, position, rotation);
                result.transform.SetParent(null);
            }

            return FinalizeRetrieve(result, pool).GetComponent<T>();
        }

        /// <summary>
        /// Returns a pooled object of the specified prefab.
        /// </summary>
        /// <param name = "prefab">Prefab to retrieve.</param>
        /// <param name = "position">Position for the retrieved object.</param>
        /// <param name = "rotation">Orientation for the retrieved object.</param>
        /// <param name = "parent">Transform to parent the retrieved object to.</param>
        /// <returns></returns>
        public static T Retrieve<T>(GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
        {
            return _instance.RetrieveInternal<T>(prefab, position, rotation, parent);
        }

        private T RetrieveInternal<T>(GameObject prefab, Vector3 position, Quaternion rotation, Transform parent)
        {
            PoolData pool;
            GameObject result = ReturnPooledObject(prefab, out pool);

            //If pool isn't null then reset result.
            if (result != null)
            {
                SetGameObjectPositionRotation(result, position, rotation);
                result.transform.SetParent(parent, true);
            }

            return FinalizeRetrieve(result, pool).GetComponent<T>();
        }

        /// <summary>
        /// Finalizes the retrieved object by performing additional reset actions before returning git.
        /// </summary>
        /// <param name = "result"></param>
        /// <param name = "pool"></param>
        /// <returns></returns>
        private GameObject FinalizeRetrieve(GameObject result, PoolData pool)
        {
            _activeObjects[result] = pool;
            //Set the item active based on if the prefab is.
            if (pool != null)
                result.SetActive(pool.Prefab.activeSelf);

            return result;
        }

        /// <summary>
        /// Returns an object to it's pool.
        /// </summary>
        /// <param name = "instantiatedObject"></param>
        /// <param name = "parentPooler">True to set the objects parent as the ObjectPooler.</param>
        public static void Store(GameObject instantiatedObject, float delay, bool parentPooler = true)
        {
            _instance.StoreInternal(instantiatedObject, delay, parentPooler);
        }

        private void StoreInternal(GameObject instantiatedObject, float delay, bool parentPooler = true)
        {
            _delayedStoreObjects[instantiatedObject] = new(Time.time + delay, parentPooler);
        }

        /// <summary>
        /// Returns an object to it's pool.
        /// </summary>
        /// <param name = "instantiatedObject"></param>
        /// <param name = "parentPooler">True to set the objects parent as the ObjectPooler.</param>
        public static void Store(GameObject instantiatedObject, bool parentPooler = true)
        {
            _instance.StoreInternal(instantiatedObject, parentPooler);
        }

        private void StoreInternal(GameObject instantiatedObject, bool parentPooler = true)
        {
            //If passed in pool object is null.
            if (instantiatedObject == null)
            {
                Debug.LogWarning("ObjectPooler -> StoreObject -> poolObject cannot be null.");
                return;
            }

            PoolData pool;
            /* Try to get the pool from the dictionary lookup.
             * If not found then create a new pool. */
            if (_activeObjects.TryGetValue(instantiatedObject, out pool))
                _activeObjects.Remove(instantiatedObject);
            else
                pool = ReturnPoolData(instantiatedObject);

            AddToPool(instantiatedObject, pool, parentPooler);
        }

        /// <summary>
        /// Makes and assigns the collector transform.
        /// </summary>
        private void MakeCollector()
        {
            if (_collector == null)
            {
                /* If collect is null it's safe to assume it's children were destroyed
                 * in which case clear dictionary used for categories. */
                _categoryChildren.Clear();
                //Make a new collector.
                _collector = new GameObject().transform;
                _collector.name = "ObjectPoolerCollector";
            }
        }

        /// <summary>
        /// Returns a poolobject without performing any operations on it.
        /// </summary>
        /// <param name = "prefab"></param>
        /// <param name = "pool">Pool the returned GameObject came from.</param>
        private GameObject ReturnPooledObject(GameObject prefab, out PoolData pool)
        {
            //If passed in prefab is null.
            if (prefab == null)
            {
                pool = null;
                Debug.LogError("ObjectPooler -> RetrieveObject -> prefab cannot be null.");
                return null;
            }

            pool = ReturnPoolData(prefab);

            GameObject result = pool.Objects.Pop();
            /* If a null object was returned then instantiate a new object and
             * use it as the result. Doing so bypasses the need to check pool
             * count, as well handles conditions in which the object may have
             * been destroyed or the pool has become corrupt. In the chance the pool was
             * corrupt or an object destroyed it will progressively clean itself with every Pop(). */
            if (result == null)
                result = Instantiate(prefab);

            return result;
        }

        /// <summary>
        /// Creates a pool of the specified prefab with one entry and adds it to the pools list.
        /// </summary>
        /// <param name = "prefab"></param>
        /// <returns>Returns the created PoolData.</returns>
        private PoolData CreatePool(GameObject prefab)
        {
            if (prefab == null)
            {
                Debug.LogError("ObjectPooler -> CreatePool -> prefab cannot be null.");
                return null;
            }

            //Make a new pool.
            PoolData pool = new(prefab, _dataExpirationDelay);

            /* Check if the poolObject has a name in the scene.
             * If it does then the object is spawned. If not
             * it must be a prefab. */
            if (prefab.scene.name != null)
                AddToPool(prefab, pool, true);

            //Add new pool to list then return it.
            _pools.Add(pool);
            _poolPrefabs[prefab] = pool;

            return pool;
        }

        /// <summary>
        /// Adds an object to a specified pool.
        /// </summary>
        /// <param name = "poolObject"></param>
        /// <param name = "pool"></param>
        /// <param name = "parentPooler">True to set the objects parent as the ObjectPooler.</param>
        private void AddToPool(GameObject instantiatedObject, PoolData pool, bool parentPooler = true)
        {
            if (instantiatedObject == null)
            {
                Debug.LogError("ObjectPooler -> AddToPool -> instantiatedObject is null.");
                return;
            }
            if (pool == null)
            {
                Debug.LogError("ObjectPooler -> AddToPool -> pool is null.");
                return;
            }

            instantiatedObject.SetActive(false);

            pool.Objects.Push(instantiatedObject);

            if (parentPooler)
                ParentPooler(instantiatedObject, true);
        }

        /// <summary>
        /// Sets the parent of the specified object to the proper child within the ObjectPooler.
        /// </summary>
        /// <param name = "obj"></param>
        private void ParentPooler(GameObject poolObject, bool worldPositionStays)
        {
            MakeCollector();

            Transform newParent;
            string tag = poolObject.tag;
            /* Try to set parent from dictionary. If not found then make a new
             * child object attached to the object pooler and add to dictionary,
             * then use created value. */
            if (!_categoryChildren.TryGetValue(tag, out newParent))
            {
                newParent = new GameObject().transform;
                newParent.name = tag;
                //Set category transform to collector.
                newParent.SetParent(_collector);
                _categoryChildren[tag] = newParent;
            }

            poolObject.transform.SetParent(newParent, worldPositionStays);
        }
    }
}