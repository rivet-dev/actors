using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using UnityEngine;
using TimeManagerCls = FishNet.Managing.Timing.TimeManager;

namespace FishNet.Component.Prediction
{
    public abstract class NetworkCollider2D : NetworkColliderBase
    {
        /// <summary>
        /// Called when another collider enters this collider.
        /// </summary>
        public event Action<Collider2D, uint> OnEnter;
        /// <summary>
        /// Called when another collider stays in this collider.
        /// </summary>
        public event Action<Collider2D, uint> OnStay;
        /// <summary>
        /// Called when another collider exits this collider.
        /// </summary>
        public event Action<Collider2D, uint> OnExit;
        /// <summary>
        /// The colliders on this object.
        /// </summary>
        private Collider2D[] _colliders;
        /// <summary>
        /// The hits from the last check.
        /// </summary>
        private Collider2D[] _hits;
        /// <summary>
        /// Colliders which are entered for a tick, be it stay or for the first time.
        /// </summary>
        private Dictionary<uint, HashSet<Collider2D>> _enteredColliders;

        protected override void Awake()
        {
            base.Awake();

            _enteredColliders = CollectionCaches<uint, HashSet<Collider2D>>.RetrieveDictionary();
            _hits = CollectionCaches<Collider2D>.RetrieveArray();
            if (_hits.Length < MaximumSimultaneousHits)
                _hits = new Collider2D[MaximumSimultaneousHits];
        }

        
        public override void OnStopNetwork()
        {
            base.OnStopNetwork();

            StoreEnteredColliders(keepDictionary: true);
            _enteredColliders?.Clear();
        }

        protected override void OnDestroy()
        {
            base.OnDestroy();

            CollectionCaches<uint, HashSet<Collider2D>>.StoreAndDefault(ref _enteredColliders);
            CollectionCaches<Collider2D>.StoreAndDefault(ref _hits, _hits.Length);
        }

        /// <summary>
        /// Called by the PredictionManager immediately before a reconcile begins.
        /// </summary>
        protected override void PredictionManager_OnPostPhysicsTransformSync(uint clientTick, uint serverTick)
        {
            if (IsStopping)
                return;

            if (clientTick > 0)
            {
                List<uint> keysToRemove = CollectionCaches<uint>.RetrieveList();

                uint maximumTick = clientTick - 2;
                foreach (uint enteredTick in _enteredColliders.Keys)
                {
                    if (enteredTick < maximumTick)
                        keysToRemove.Add(enteredTick);
                }

                foreach (uint tick in keysToRemove)
                {
                    HashSet<Collider2D> colliders = _enteredColliders[tick];
                    CollectionCaches<Collider2D>.Store(colliders);

                    _enteredColliders.Remove(tick);
                }

                CollectionCaches<uint>.Store(keysToRemove);
            }

            /* Call base only after removing old entries. This ensures old entries are removed
             * before CheckColliders is called. */
            base.PredictionManager_OnPostPhysicsTransformSync(clientTick, serverTick);
        }

        /// <summary>
        /// Checks for any collider changes;
        /// </summary>
        protected override void CheckColliders(uint localTick)
        {
                // Initial checks failed.
            if (!TryPrepareColliderCheck(localTick))
                return;

            HashSet<Collider2D> current = CollectionCaches<Collider2D>.RetrieveHashSet();

            /* Previous may not be set here if there were
             * no collisions during the previous tick. */

            // The rotation of the object for box colliders.
            Quaternion rotation = transform.rotation;

            // Check each collider for triggers.
            foreach (Collider2D col in _colliders)
            {
                if (!col.enabled)
                    continue;
                if (IsTrigger != col.isTrigger)
                    continue;

                // Number of hits from the checks.
                // Number of hits from the checks.
                int hits;
                if (col is CircleCollider2D circleCollider)
                    hits = GetCircleCollider2DHits(circleCollider, InteractableLayers);
                else if (col is BoxCollider2D boxCollider)
                    hits = GetBoxCollider2DHits(boxCollider, rotation, InteractableLayers);
                else
                    hits = 0;

                /* Check hits for enter/exit callbacks. */
                for (int i = 0; i < hits; i++)
                {
                    Collider2D hit = _hits[i];
                    if (hit == null || hit == col)
                        continue;

                    current.Add(hit);
                }

                /* If the colliders already exist then the tick is being
                 * run again, which would indicate this is being run during a reconcile.
                 *
                 * Since this key will have its data replaced with current, store the prior collection.*/
                if (_enteredColliders.TryGetValueIL2CPP(localTick, out HashSet<Collider2D> enteredColliders))
                {
                    CollectionCaches<Collider2D>.Store(enteredColliders);
                    _enteredColliders.Remove(localTick);
                }

                const uint unsetLastTick = uint.MaxValue;
                uint lastTick = localTick > 1 ? localTick - 1 : unsetLastTick;

                _enteredColliders.TryGetValueIL2CPP(lastTick, out HashSet<Collider2D> lastEnteredColliders);

                /* If there are entered colliders then
                 * update enteredColliders for the tick. */
                if (current.Count > 0)
                {
                    _enteredColliders[localTick] = current;

                    /* If there were no colliders last tick
                     * then without a doubt enter should be called since
                     * the collider could not possibly be present already. */
                    if (lastEnteredColliders == null)
                    {
                        //Invoke OnEnter for every collider in current.
                        foreach (Collider2D c in current)
                            OnEnter?.Invoke(c, localTick);
                    }
                    /* If the last collection is found then
                     * check to invoke Enter or Stay. */
                    else
                    {
                        foreach (Collider2D c in current)
                        {
                            if (lastEnteredColliders.Contains(c))
                                OnStay?.Invoke(c, localTick);
                            else
                                OnEnter?.Invoke(c, localTick);
                        }
                    }
                }
                //If current is empty the collection can be stored.
                else
                {
                    CollectionCaches<Collider2D>.Store(current);
                }

                /* Check to invoke OnExit. */
                if (lastEnteredColliders != null)
                {
                    /* If current does not have the colliders from
                     * the last tick, then an exit has occurred. */
                    foreach (Collider2D c in lastEnteredColliders)
                    {
                        if (!current.Contains(c))
                            OnExit?.Invoke(c, localTick);
                    }
                }

                /* If the server is started the lastEnteredColliders can
                 * be discarded since the server will never reconcile, and
                 * will never need to check them again. */
                if (IsServerStarted)
                {
                    if (lastTick is not unsetLastTick && _enteredColliders.TryGetValueIL2CPP(localTick, out HashSet<Collider2D> lEnteredColliders))
                    {
                        CollectionCaches<Collider2D>.Store(lEnteredColliders);
                        _enteredColliders.Remove(localTick);
                    }
                }
            }
        }

        /// <summary>
        /// Checks for circle collisions.
        /// </summary>
        /// <returns>Number of colliders hit.</returns>
        private int GetCircleCollider2DHits(CircleCollider2D circleCollider, int layerMask)
        {
            circleCollider.GetCircleOverlapParams(out Vector3 center, out float radius);
            radius += AdditionalSize;
            return gameObject.scene.GetPhysicsScene2D().OverlapCircle(center, radius, _hits, layerMask);
        }

        /// <summary>
        /// Checks for Box collisions.
        /// </summary>
        /// <returns>Number of colliders hit.</returns>
        private int GetBoxCollider2DHits(BoxCollider2D boxCollider, Quaternion rotation, int layerMask)
        {
            boxCollider.GetBox2DOverlapParams(out Vector3 center, out Vector3 halfExtents);
            Vector3 additional = Vector3.one * AdditionalSize;
            halfExtents += additional;
            return gameObject.scene.GetPhysicsScene2D().OverlapBox(center, halfExtents, rotation.z, _hits, layerMask);
        }

        /// <summary>
        /// Finds colliders on this object to check.
        /// </summary>
        /// <param name = "force">True to set colliders again even if already found. This action will clear stored collider states.</param>
        /// <returns>True if colliders should be found again.</returns>
        public override bool TryFindColliders(bool force = false)
        {
            if (!base.TryFindColliders(force))
                return false;

            ClearColliderDataHistory(invokeOnExit: true);
            _colliders = GetComponents<Collider2D>();

            return true;
        }

        /// <summary>
        /// Resets this NetworkBehaviour so that it may be added to an object pool.
        /// </summary>
        public override void ResetState(bool asServer)
        {
            ClearColliderDataHistory(invokeOnExit: true);
            base.ResetState(asServer);
        }

        /// <summary>
        /// Clears stored collider states.
        /// </summary>
        /// <param name = "invokeOnExit">True to invoke OnExit if a collider is stored in the OnEntered state. When called during a reconcile this used the current ClientReplayTick, otherwise uses LocalTick.</param>
        protected override void ClearColliderDataHistory(bool invokeOnExit)
        {
            if (_enteredColliders == null)
                return;

            /* Data needs to exist to iterate, and managers are needed
             * to get the proper tick to invoke. */
            if (invokeOnExit)
            {
                uint largestTick = 0;
                foreach (uint tick in _enteredColliders.Keys)
                    largestTick = Math.Max(tick, largestTick);
                
                if (_enteredColliders.TryGetValueIL2CPP(largestTick, out HashSet<Collider2D> colliders))
                {
                    if (colliders != null)
                    {
                        foreach (Collider2D c in colliders)
                            OnExit?.Invoke(c, TimeManagerCls.UNSET_TICK);
                    }
                }
            }

            StoreEnteredColliders(keepDictionary: true);
            _enteredColliders.Clear();
        }

        /// <summary>
        /// Stores each Collider HashSet within EnteredColliders.
        /// </summary>
        private void StoreEnteredColliders(bool keepDictionary)
        {
            foreach (HashSet<Collider2D> colliders in _enteredColliders.Values)
                CollectionCaches<Collider2D>.Store(colliders);

            if (!keepDictionary)
                CollectionCaches<uint, HashSet<Collider2D>>.Store(_enteredColliders);
        }
    }
}