// using FishNet.Connection;
// using FishNet.Documenting;
// using FishNet.Managing.Server;
// using FishNet.Object;
// using FishNet.Transporting;
// using FishNet.Utility.Performance;
// using GameKit.Dependencies.Utilities;
// using System.Collections.Generic;
// using System.Runtime.CompilerServices;
// using FishNet.Managing.Observing;
// using UnityEngine;
// using UnityEngine.Serialization;
//
// namespace FishNet.Observing
// {
//     /// <summary>
//     /// Controls which clients can see and get messages for an object.
//     /// </summary>
//     [DisallowMultipleComponent]
//     [RequireComponent(typeof(NetworkObject))]
//     [AddComponentMenu("FishNet/Component/NetworkObserver")]
//     public sealed class NetworkObserver : MonoBehaviour
//     {
//         #region Types.
//         /// <summary>
//         /// How ObserverManager conditions are used.
//         /// </summary>
//         public enum ConditionOverrideType
//         {
//             /// <summary>
//             /// Keep current conditions, add new conditions from manager.
//             /// </summary>
//             AddMissing = 1,
//             /// <summary>
//             /// Replace current conditions with manager conditions.
//             /// </summary>
//             UseManager = 2,
//             /// <summary>
//             /// Keep current conditions, ignore manager conditions.
//             /// </summary>
//             IgnoreManager = 3,
//         }
//         #endregion
//
//         #region Serialized.
//         /// <summary>
//         /// 
//         /// </summary>
//         [Tooltip("How ObserverManager conditions are used.")]
//         [SerializeField]
//         private ConditionOverrideType _overrideType = ConditionOverrideType.IgnoreManager;
//         /// <summary>
//         /// How ObserverManager conditions are used.
//         /// </summary>
//         public ConditionOverrideType OverrideType
//         {
//             get => _overrideType;
//             internal set => _overrideType = value;
//         }
//
//         /// <summary>
//         /// 
//         /// </summary>
//         [Tooltip("True to update visibility for clientHost based on if they are an observer or not.")]
//         [SerializeField]
//         private bool _updateHostVisibility = true;
//         /// <summary>
//         /// True to update visibility for clientHost based on if they are an observer or not.
//         /// </summary>
//         public bool UpdateHostVisibility
//         {
//             get => _updateHostVisibility;
//             private set => _updateHostVisibility = value;
//         }
//         /// <summary>
//         /// 
//         /// </summary>
//         [Tooltip("Conditions connections must met to be added as an observer. Multiple conditions may be used.")]
//         [SerializeField]
//         internal List<ObserverCondition> _observerConditions = new();
//         /// <summary>
//         /// Conditions connections must met to be added as an observer. Multiple conditions may be used.
//         /// </summary>
//         public IReadOnlyList<ObserverCondition> ObserverConditions => _observerConditions;
//         [APIExclude]
// #if MIRROR
//         public List<ObserverCondition> ObserverConditionsInternal
// #else
//         internal List<ObserverCondition> ObserverConditionsInternal
// #endif
//         {
//             get => _observerConditions;
//             set => _observerConditions = value;
//         }
//         #endregion
//
//         #region Private.
//         /// <summary>
//         /// Becomes true if there are timed conditions.
//         /// </summary>
//         private bool _hasTimedConditions;
//         /// <summary>
//         /// Connections which have all non-timed conditions met.
//         /// </summary>
//         private HashSet<NetworkConnection> _nonTimedMet;
//         /// <summary>
//         /// NetworkObject this belongs to.
//         /// </summary>
//         private NetworkObject _networkObject;
//         /// <summary>
//         /// Becomes true when registered with ServerObjects as Timed observers.
//         /// </summary>
//         private bool _registeredAsTimed;
//         /// <summary>
//         /// True if was initialized previously.
//         /// </summary>
//         private bool _initializedPreviously;
//         /// <summary>
//         /// True if currently initialized.
//         /// </summary>
//         private bool _initialized;
//         /// <summary>
//         /// Last ObserverManager hash which initialized conditions.
//         /// </summary>
//         private uint _initializingHash = ObserverManager.UNSET_INITIALIZING_HASH
//         /// <summary>
//         /// True if ParentNetworkObject was visible last iteration.
//         /// This value will also be true if there is no ParentNetworkObject.
//         /// </summary>
//         private bool _lastParentVisible;
//         /// <summary>
//         /// ServerManager for this script.
//         /// </summary>
//         private ServerManager _serverManager;
//         /// <summary>
//         /// Becomes true if there are non-timed, normal conditions.
//         /// </summary>
//         private bool _hasNormalConditions;
//         /// <summary>
//         /// ObserverConditions which are referenced or instantiated from ObserverConditions.
//         /// </summary>
//         private List<ObserverCondition> _runtimeObserverConditions;
//         #endregion
//
//         /// <summary>
//         /// Deinitializes for reuse or clean up.
//         /// </summary>
//         /// <param name="destroyed"></param>
//         internal void Deinitialize(bool destroyed)
//         {
//             if (!_initialized)
//                 return;
//
//             Debug.Log($"Deinit called on {GetInstanceID()}. Destroyed? {destroyed}");
//
//             _lastParentVisible = false;
//             if (_nonTimedMet != null)
//                 _nonTimedMet.Clear();
//             UnregisterTimedConditions();
//
//             if (_serverManager != null)
//                 _serverManager.OnRemoteConnectionState -= ServerManager_OnRemoteConnectionState;
//
//             if (_initializedPreviously)
//             {
//                 _hasNormalConditions = false;
//
//                 foreach (ObserverCondition item in _observerConditions)
//                 {
//                     item.Deinitialize(destroyed);
//                     /* Use GetInstanceId to ensure the object is actually
//                      * instantiated. If Id is negative, then it's instantiated
//                      * and not a reference to the original object. */
//                     if (destroyed && item.GetInstanceID() < 0)
//                         Destroy(item);
//                 }
//
//                 // Clean up lists.
//                 if (destroyed)
//                     CollectionCaches<NetworkConnection>.Store(_nonTimedMet);
//             }
//
//             _serverManager = null;
//             _networkObject = null;
//             _initialized = false;
//         }
//
//         /// <summary>
//         /// Initializes this script for use.
//         /// </summary>
//         internal void Initialize(NetworkObject networkObject)
//         {
//             if (_initialized)
//                 return;
//
//             Debug.Log($"Init called on {GetInstanceID()}. Initialized previously? {_initializedPreviously}");
//             _networkObject = networkObject;
//             _serverManager = _networkObject.ServerManager;
//             _serverManager.OnRemoteConnectionState += ServerManager_OnRemoteConnectionState;
//
//             if (!_initializedPreviously)
//             {
//                 _initializedPreviously = true;
//                 bool ignoringManager = (OverrideType == ConditionOverrideType.IgnoreManager);
//
//                 //Check to override SetHostVisibility.
//                 if (!ignoringManager)
//                     UpdateHostVisibility = networkObject.ObserverManager.UpdateHostVisibility;
//
//                 /* Sort the conditions so that normal conditions are first.
//                  * This prevents normal conditions from being skipped if a timed
//                  * condition fails before the normal passed.
//                  *
//                  * Example: Let's say an object has a distance and scene condition, with
//                  * the distance condition being first. Normal conditions are only checked
//                  * as the change occurs, such as when the scene was loaded. So if the client
//                  * loaded into the scene and they were not within the distance the condition
//                  * iterations would skip remaining, which would be the scene condition. As
//                  * result normal conditions (non timed) would never be met since they are only
//                  * checked as-needed, in this case during a scene change.
//                  *
//                  * By moving normal conditions to the front they will always be checked first
//                  * and timed can update at intervals per expectancy. This could also be resolved
//                  * by simply not exiting early when a condition fails but that's going to
//                  * cost hotpath performance where sorting is only done once. */
//
//                 //Initialize collections.
//                 _nonTimedMet = CollectionCaches<NetworkConnection>.RetrieveHashSet();
//                 //Caches for ordering.
//                 List<ObserverCondition> nonTimedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//                 List<ObserverCondition> timedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//
//                 bool observerFound = false;
//                 foreach (ObserverCondition condition in _observerConditions)
//                 {
//                     if (condition == null)
//                         continue;
//
//                     observerFound = true;
//
//                     /* Make an instance of each condition so values are
//                      * not overwritten when the condition exist more than
//                      * once in the scene. Double-edged sword of using scriptable
//                      * objects for conditions. */
//                     ObserverCondition ocCopy = Instantiate(condition);
//
//                     //Condition type.
//                     ObserverConditionType oct = ocCopy.GetConditionType();
//                     if (oct == ObserverConditionType.Timed)
//                     {
//                         timedConditions.AddOrdered(ocCopy);
//                     }
//                     else
//                     {
//                         _hasNormalConditions = true;
//                         nonTimedConditions.AddOrdered(ocCopy);
//                     }
//                 }
//
//                 //Add to condition collection as ordered now.
//                 _observerConditions.Clear();
//                 //Non timed.
//                 for (int i = 0; i < nonTimedConditions.Count; i++)
//                     _observerConditions.Add(nonTimedConditions[i]);
//
//                 //Timed.
//                 _timedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//                 foreach (ObserverCondition timedCondition in timedConditions)
//                 {
//                     _observerConditions.Add(timedCondition);
//                     _timedConditions.Add(timedCondition);
//                 }
//
//                 //Store caches.
//                 CollectionCaches<ObserverCondition>.Store(nonTimedConditions);
//                 CollectionCaches<ObserverCondition>.Store(timedConditions);
//
//                 //No observers specified, do not need to take further action.
//                 if (!observerFound)
//                     return;
//             }
//
//             //Initialize conditions.
//             for (int i = 0; i < _observerConditions.Count; i++)
//                 _observerConditions[i].Initialize(_networkObject);
//
//             _initialized = true;
//
//             RegisterTimedConditions();
//         }
//
//         /// <summary>
//         /// Returns a condition if found within Conditions.
//         /// </summary>
//         /// <returns></returns>
//         public ObserverCondition GetObserverCondition<T>() where T : ObserverCondition
//         {
//             /* Do not bother setting local variables,
//              * condition collections aren't going to be long
//              * enough to make doing so worth while. */
//
//             System.Type conditionType = typeof(T);
//             for (int i = 0; i < _observerConditions.Count; i++)
//             {
//                 if (_observerConditions[i].GetType() == conditionType)
//                     return _observerConditions[i];
//             }
//
//             //Fall through, not found.
//             return null;
//         }
//
//         /// <summary>
//         /// Returns ObserverStateChange by comparing conditions for a connection.
//         /// </summary>
//         /// <returns>True if added to Observers.</returns>
//         internal ObserverStateChange RebuildObservers(NetworkConnection connection, bool timedOnly)
//         {
//             bool currentlyAdded = (_networkObject.Observers.Contains(connection));
//
//             //True if all conditions are met.
//             bool allConditionsMet = true;
//             /* If cnnection is owner then they can see the object. */
//             bool notOwner = (connection != _networkObject.Owner);
//             /* Only check conditions if not owner. Owner will always
//              * have visibility. */
//             if (notOwner)
//             {
//                 bool parentVisible = true;
//                 if (_networkObject.CurrentParentNetworkBehaviour != null)
//                     parentVisible = _networkObject.CurrentParentNetworkBehaviour.NetworkObject.Observers.Contains(connection);
//
//                 /* If parent is visible but was not previously
//                  * then unset timedOnly to make sure all conditions
//                  * are checked again. This ensures that the _nonTimedMet
//                  * collection is updated. */
//                 if (parentVisible && !_lastParentVisible)
//                     timedOnly = false;
//                 _lastParentVisible = parentVisible;
//
//                 //If parent is not visible no further checks are required.
//                 if (!parentVisible)
//                 {
//                     allConditionsMet = false;
//                 }
//                 //Parent is visible, perform checks.
//                 else
//                 {
//                     //Only need to check beyond this if conditions exist.
//                     if (_observerConditions.Count > 0)
//                     {
//                         /* True if all conditions are timed or
//                          * if connection has met non timed. */
//                         bool startNonTimedMet = (!_hasNormalConditions || _nonTimedMet.Contains(connection));
//                         /* If a timed update an1d nonTimed
//                          * have not been met then there's
//                          * no reason to check timed. */
//                         if (timedOnly && !startNonTimedMet)
//                         {
//                             allConditionsMet = false;
//                         }
//                         else
//                         {
//                             //Becomes true if a non-timed condition fails.
//                             bool nonTimedMet = true;
//
//                             List<ObserverCondition> collection = _runtimeObserverConditions;
//                             for (int i = 0; i < collection.Count; i++)
//                             {
//                                 ObserverCondition condition = collection[i];
//
//                                 if (timedOnly && condition.GetConditionType() != ObserverConditionType.Timed)
//                                     continue;
//                                 /* If any observer returns removed then break
//                                  * from loop and return removed. If one observer has
//                                  * removed then there's no reason to iterate
//                                  * the rest.
//                                  *
//                                  * A condition is automatically met if it's not enabled. */
//                                 bool notProcessed = false;
//                                 Debug.LogWarning($"Condition check {GetInstanceID()}     {condition.GetInstanceID()}. Type {condition.GetType()}");
//                                 bool conditionMet = (!condition.GetIsEnabled() || condition.ConditionMet(connection, currentlyAdded, out notProcessed));
//
//                                 if (notProcessed)
//                                     conditionMet = currentlyAdded;
//
//                                 //Condition not met.
//                                 if (!conditionMet)
//                                 {
//                                     allConditionsMet = false;
//                                     if (condition.GetConditionType() != ObserverConditionType.Timed)
//                                         nonTimedMet = false;
//                                     break;
//                                 }
//                             }
//
//                             //If nonTimedMet changed.
//                             if (startNonTimedMet != nonTimedMet)
//                             {
//                                 /* If the collection was iterated without breaks
//                                  * then add to nontimed met. */
//                                 if (nonTimedMet)
//                                     _nonTimedMet.Add(connection);
//                                 //If there were breaks not all conditions were checked.
//                                 else
//                                     _nonTimedMet.Remove(connection);
//                             }
//                         }
//                     }
//                 }
//             }
//
//             //If all conditions met.
//             if (allConditionsMet)
//                 return ReturnPassedConditions(currentlyAdded);
//             else
//                 return ReturnFailedCondition(currentlyAdded);
//         }
//
//         /// <summary>
//         /// Registers timed observer conditions.
//         /// </summary>
//         private void RegisterTimedConditions()
//         {
//             if (!_hasTimedConditions)
//                 return;
//             if (_registeredAsTimed)
//                 return;
//             
//             _registeredAsTimed = true;
//
//             if (_serverManager == null)
//                 return;
//             _serverManager.Objects.AddTimedNetworkObserver(_networkObject);
//         }
//
//         /// <summary>
//         /// Unregisters timed conditions.
//         /// </summary>
//         private void UnregisterTimedConditions()
//         {
//             if (!_hasTimedConditions)
//                 return;
//             if (!_registeredAsTimed)
//                 return;
//             _registeredAsTimed = false;
//
//             if (_serverManager == null)
//                 return;
//             _serverManager.Objects.RemoveTimedNetworkObserver(_networkObject);
//         }
//
//         /// <summary>
//         /// Returns an ObserverStateChange when a condition fails.
//         /// </summary>
//         /// <param name="currentlyAdded"></param>
//         /// <returns></returns>
//         private ObserverStateChange ReturnFailedCondition(bool currentlyAdded)
//         {
//             if (currentlyAdded)
//                 return ObserverStateChange.Removed;
//             else
//                 return ObserverStateChange.Unchanged;
//         }
//
//         /// <summary>
//         /// Returns an ObserverStateChange when all conditions pass.
//         /// </summary>
//         /// <param name="currentlyAdded"></param>
//         /// <returns></returns>
//         private ObserverStateChange ReturnPassedConditions(bool currentlyAdded)
//         {
//             if (currentlyAdded)
//                 return ObserverStateChange.Unchanged;
//             else
//                 return ObserverStateChange.Added;
//         }
//
//         /// <summary>
//         /// Called when a remote client state changes with the server.
//         /// </summary>
//         private void ServerManager_OnRemoteConnectionState(NetworkConnection conn, RemoteConnectionStateArgs arg2)
//         {
//             if (arg2.ConnectionState == RemoteConnectionState.Stopped)
//                 _nonTimedMet.Remove(conn);
//         }
//
//         /// <summary>
//         /// Updates current conditions to supplied values. Conditions are updated if forced or if the manager hash provided differs from what is stored.
//         /// </summary>
//         internal void SetObserverConditions(List<ObserverCondition> conditions, uint observerManagerHash, bool force = false)
//         {
//             //Already initialized for the observerManager!
//             if (observerManagerHash == _initializingHash && !force)
//                 return;
//             if (_overrideType == ConditionOverrideType.IgnoreManager)
//                 return;
//
//             DisposeOfRuntimeConditions();
//
//             //Caches for ordering.
//             List<ObserverCondition> nonTimedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//             List<ObserverCondition> timedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//
//             //Set new conditions.
//             foreach (ObserverCondition oc in conditions)
//             {
//                 if (oc == null)
//                     continue;
//
//                 /* Make an instance of each condition so values are
//                  * not overwritten when the condition exist more than
//                  * once in the scene. Double-edged sword of using scriptable
//                  * objects for conditions. */
//                 ObserverCondition ocCopy = (oc.IsConstant) ? oc : Instantiate(oc);
//
//                 //Condition type.
//                 ObserverConditionType oct = ocCopy.GetConditionType();
//                 if (oct == ObserverConditionType.Timed)
//                 {
//                     timedConditions.AddOrdered(ocCopy);
//                 }
//                 else
//                 {
//                     _hasNormalConditions = true;
//                     nonTimedConditions.AddOrdered(ocCopy);
//                 }
//             }
//
//             CollectionCaches<ObserverCondition>.StoreAndDefault(ref _runtimeObserverConditions);
//             _runtimeObserverConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//
//             /* Add nonTimed first, as they're always checked first for performance. */
//             for (int i = 0; i < nonTimedConditions.Count; i++)
//                 _runtimeObserverConditions.Add(nonTimedConditions[i]);
//
//             /* Add timed into their own collection as well runtime collection.
//              * There are separate collections as timed are checked regularly so
//              * this prevents iterating over all conditions in a timed check. */
//             CollectionCaches<ObserverCondition>.StoreAndDefault(ref _timedConditions);
//             _timedConditions = CollectionCaches<ObserverCondition>.RetrieveList();
//             foreach (ObserverCondition timedCondition in timedConditions)
//             {
//                 _observerConditions.Add(timedCondition);
//                 _timedConditions.Add(timedCondition);
//             }
//
//             //Store caches.
//             CollectionCaches<ObserverCondition>.Store(nonTimedConditions);
//             CollectionCaches<ObserverCondition>.Store(timedConditions);
//         }
//
//         /// <summary>
//         /// Updates current conditions to supplied values. Conditions are updated if forced or if the manager hash provided differs from what is stored.
//         /// </summary>
//         public void UpdateObserverConditions(List<ObserverCondition> conditions, uint observerManagerHash, bool force = false)
//         {
//             //Already initialized for the observerManager!
//             if (observerManagerHash == _initializingHash && !force)
//                 return;
//
//             //Dispose of current instantiated conditions.
//             foreach (ObserverCondition oc in _observerConditions)
//             {
//                 //Constant are never initialized.
//                 if (oc.IsConstant)
//                     continue;
//                 /* Not constant, but isn't in instance.
//                  * Unity tells us only negative Ids are instantiated. */
//                 if (oc.GetInstanceID() >= 0)
//                     continue;
//
//                 oc.Deinitialize(destroyed: true);
//                 Destroy(oc);
//             }
//
//             _observerConditions.Clear();
//         }
//
//         /// <summary>
//         /// Destroys runtime ObserverConditions as needed and clears the collection.
//         /// </summary>
//         private void DisposeOfRuntimeConditions()
//         {
//             if (_runtimeObserverConditions == null)
//                 return;
//
//             foreach (ObserverCondition oc in _runtimeObserverConditions)
//             {
//                 //Constant are never initialized.
//                 if (oc.IsConstant)
//                     continue;
//                 /* Not constant, but isn't in instance.
//                  * Unity tells us only negative Ids are instantiated. */
//                 if (oc.GetInstanceID() >= 0)
//                     continue;
//
//                 oc.Deinitialize(destroyed: true);
//                 Destroy(oc);
//             }
//
//             _runtimeObserverConditions.Clear();
//         }
//
//         /// <summary>
//         /// Sets a new value for UpdateHostVisibility.
//         /// This does not immediately update renderers.
//         /// You may need to combine with NetworkObject.SetRenderersVisible(bool).
//         /// </summary>
//         /// <param name="value">New value.</param>
//         public void SetUpdateHostVisibility(bool value)
//         {
//             //Unchanged.
//             if (value == UpdateHostVisibility)
//                 return;
//
//             UpdateHostVisibility = value;
//         }
//     }
// }

