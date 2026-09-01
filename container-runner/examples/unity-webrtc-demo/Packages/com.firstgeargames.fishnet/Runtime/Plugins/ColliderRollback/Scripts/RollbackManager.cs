#if !FISHNET_THREADED_COLLIDER_ROLLBACK
using FishNet.Connection;
using FishNet.Managing;
using FishNet.Managing.Scened;
using FishNet.Managing.Timing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using Unity.Profiling;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace FishNet.Component.ColliderRollback
{
    public class RollbackManager : MonoBehaviour
    {
        #region Internal.
        /// <summary>
        /// Cached value for bounding box layermask.
        /// </summary>
        internal int? BoundingBoxLayerNumber
        {
            get
            {
                if (_boundingBoxLayerNumber == null)
                {
                    for (int i = 0; i < 32; i++)
                    {
                        if (1 << i == BoundingBoxLayer.value)
                        {
                            _boundingBoxLayerNumber = i;
                            break;
                        }
                    }
                }

                return _boundingBoxLayerNumber;
            }
        }
        private int? _boundingBoxLayerNumber;
        #endregion

        #region Serialized.
        /// <summary>
        /// </summary>
        [Tooltip("Layer to use when creating and checking against bounding boxes. This should be different from any layer used.")]
        [SerializeField]
        private LayerMask _boundingBoxLayer = 0;
        /// <summary>
        /// Layer to use when creating and checking against bounding boxes. This should be different from any layer used.
        /// </summary>
        internal LayerMask BoundingBoxLayer => _boundingBoxLayer;
        /// <summary>
        /// </summary>
        [Tooltip("Maximum time in the past colliders can be rolled back to.")]
        [SerializeField]
        private float _maximumRollbackTime = 1.25f;
        /// <summary>
        /// Maximum time in the past colliders can be rolled back to.
        /// </summary>
        internal float MaximumRollbackTime => _maximumRollbackTime;
        /// <summary>
        /// </summary>
        [Tooltip("Interpolation value for the NetworkTransforms or objects being rolled back.")]
        [Range(0, 250)]
        [SerializeField]
        internal ushort Interpolation = 2;
        #endregion



        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        /// <param name = "manager"></param>
        internal void InitializeOnce_Internal(NetworkManager manager)
        {
        }



        [Obsolete("Use Rollback(Vector3, Vector3, float, PreciseTick, RollbackPhysicsType.Physics, bool) instead.")] //Remove on V5
        public void Rollback(Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, bool asOwnerAndClientHost = false)
        {
        }

        [Obsolete("Use Rollback(Scene, Vector3, Vector3, float, PreciseTick, RollbackPhysicsType.Physics, bool) instead.")] //Remove on V5
        public void Rollback(Scene scene, Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, bool asOwnerAndClientHost = false)
        {
        }

        [Obsolete("Use Rollback(int, Vector3, Vector3, float, PreciseTick, RollbackPhysicsType.Physics, bool) instead.")] //Remove on V5
        public void Rollback(int sceneHandle, Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, bool asOwnerAndClientHost = false)
        {
        }

        [Obsolete("Use Rollback(Scene, Vector3, Vector3, float, PreciseTick, RollbackPhysicsType.Physics2D, bool) instead.")] //Remove on V5
        public void Rollback(Scene scene, Vector2 origin, Vector2 normalizedDirection, float distance, PreciseTick pt, bool asOwnerAndClientHost = false)
        {
        }

        [Obsolete("Use Rollback(Vector3, Vector3, float, PreciseTick, RollbackPhysicsType.Physics2D, bool) instead.")] //Remove on V5
        public void Rollback(Vector2 origin, Vector2 normalizedDirection, float distance, PreciseTick pt, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back all colliders.
        /// </summary>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back all colliders in a scene.
        /// </summary>
        /// <param name = "scene">Scene containing colliders.</param>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(Scene scene, PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back all colliders in a scene.
        /// </summary>
        /// <param name = "sceneHandle">Scene handle containing colliders.</param>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(int sceneHandle, PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back colliders hit by a test cast against bounding boxes.
        /// </summary>
        /// <param name = "origin">Ray origin.</param>
        /// <param name = "normalizedDirection">Direction to cast.</param>
        /// <param name = "distance">Distance of cast.</param>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back colliders hit by a test cast against bounding boxes, in a specific scene.
        /// </summary>
        /// <param name = "scene">Scene containing colliders.</param>
        /// <param name = "origin">Ray origin.</param>
        /// <param name = "normalizedDirection">Direction to cast.</param>
        /// <param name = "distance">Distance of cast.</param>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(Scene scene, Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }

        /// <summary>
        /// Rolls back colliders hit by a test cast against bounding boxes, in a specific scene.
        /// </summary>
        /// <param name = "sceneHandle">Scene handle containing colliders.</param>
        /// <param name = "origin">Ray origin.</param>
        /// <param name = "normalizedDirection">Direction to cast.</param>
        /// <param name = "distance">Distance of cast.</param>
        /// <param name = "pt">Precise tick received from the client.</param>
        /// <param name = "physicsType">Type of physics to rollback; this is often what your casts will use.</param>
        /// <param name = "asOwnerAndClientHost">True if IsOwner of the object the raycast is for. This can be ignored and only provides more accurate results for clientHost.</param>
        public void Rollback(int sceneHandle, Vector3 origin, Vector3 normalizedDirection, float distance, PreciseTick pt, RollbackPhysicsType physicsType, bool asOwnerAndClientHost = false)
        {
        }


        /// <summary>
        /// Returns all ColliderRollback objects back to their original position.
        /// </summary>
        public void Return()
        {
        }

    }
}
#endif