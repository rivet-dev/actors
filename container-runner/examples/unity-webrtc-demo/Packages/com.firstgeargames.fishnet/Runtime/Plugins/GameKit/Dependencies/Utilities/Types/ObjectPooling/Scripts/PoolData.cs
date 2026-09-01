using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.ObjectPooling
{
    public class PoolData
    {
        public PoolData(GameObject prefab, float expirationDuration)
        {
            Prefab = prefab;
            _expirationDuration = expirationDuration;
        }

        #region Public.
        /// <summary>
        /// Prefab for this pool.
        /// </summary>
        public readonly GameObject Prefab = null;
        /// <summary>
        /// Objects currently in the pool.
        /// </summary>
        public ListStack<GameObject> Objects = new();
        #endregion

        #region Private.
        /// <summary>
        /// Time this pool must remain idle before it's considered expired.
        /// </summary>
        private float _expirationDuration = -1f;
        #endregion

        /// <summary>
        /// Returns if the pool has expired due to inactivity.
        /// </summary>
        /// <param name = "expirationDuration"></param>
        /// <returns></returns>
        public bool PoolExpired()
        {
            if (_expirationDuration == -1f)
                return false;

            return !Objects.AccessedRecently(_expirationDuration);
        }

        /// <summary>
        /// Returns a list of GameObjects which were culled from the stack using the default expiration duration.
        /// </summary>
        /// <param name = "threshold"></param>
        /// <returns></returns>
        public List<GameObject> Cull()
        {
            if (_expirationDuration == -1f)
                return new();

            return Objects.Cull(_expirationDuration);
        }
    }
}