using FishNet.CodeGenerating;
using FishNet.Component.Prediction;
using FishNet.Managing;
using FishNet.Serializing;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.Scripting;

namespace FishNet.Object.Prediction
{
    [Preserve]
    [DefaultWriter]
    public static class PredictionRigidbodySerializers
    {
        [DefaultWriter]
        public static void WriteEntryData(this Writer w, PredictionRigidbody.EntryData value)
        {
            PredictionRigidbody.ForceApplicationType appType = value.Type;
            w.WriteUInt8Unpacked((byte)appType);
            PredictionRigidbody.AllForceData data = value.Data;

            switch (appType)
            {
                case PredictionRigidbody.ForceApplicationType.AddTorque:
                case PredictionRigidbody.ForceApplicationType.AddForce:
                case PredictionRigidbody.ForceApplicationType.AddRelativeTorque:
                case PredictionRigidbody.ForceApplicationType.AddRelativeForce:
                    w.WriteVector3(data.Vector3Force);
                    w.WriteInt32((byte)data.Mode);
                    break;
                case PredictionRigidbody.ForceApplicationType.AddExplosiveForce:
                    w.WriteSingle(data.FloatForce);
                    w.WriteVector3(data.Position);
                    w.WriteSingle(data.Radius);
                    w.WriteSingle(data.UpwardsModifier);
                    w.WriteInt32((byte)data.Mode);
                    break;
                case PredictionRigidbody.ForceApplicationType.AddForceAtPosition:
                    w.WriteVector3(data.Vector3Force);
                    w.WriteVector3(data.Position);
                    w.WriteInt32((byte)data.Mode);
                    break;
                case PredictionRigidbody.ForceApplicationType.MovePosition:
                    w.WriteVector3(data.Position);
                    break;
                case PredictionRigidbody.ForceApplicationType.MoveRotation:
                    w.WriteUInt8Unpacked((byte)data.RotationPacking);
                    w.WriteQuaternion(data.Rotation, data.RotationPacking);
                    break;
                default:
                    w.NetworkManager.LogError($"ForceApplicationType of {appType} is not supported.");
                    break;
            }
        }

        [DefaultReader]
        public static PredictionRigidbody.EntryData ReadEntryData(this Reader r)
        {
            PredictionRigidbody.EntryData fd = new();

            PredictionRigidbody.ForceApplicationType appType = (PredictionRigidbody.ForceApplicationType)r.ReadUInt8Unpacked();
            fd.Type = appType;

            PredictionRigidbody.AllForceData data = new();

            switch (appType)
            {
                case PredictionRigidbody.ForceApplicationType.AddTorque:
                case PredictionRigidbody.ForceApplicationType.AddForce:
                case PredictionRigidbody.ForceApplicationType.AddRelativeTorque:
                case PredictionRigidbody.ForceApplicationType.AddRelativeForce:
                    data.Vector3Force = r.ReadVector3();
                    data.Mode = (ForceMode)r.ReadInt32();
                    break;
                case PredictionRigidbody.ForceApplicationType.AddExplosiveForce:
                    data.FloatForce = r.ReadSingle();
                    data.Position = r.ReadVector3();
                    data.Radius = r.ReadSingle();
                    data.UpwardsModifier = r.ReadSingle();
                    data.Mode = (ForceMode)r.ReadInt32();
                    break;
                case PredictionRigidbody.ForceApplicationType.AddForceAtPosition:
                    data.Vector3Force = r.ReadVector3();
                    data.Position = r.ReadVector3();
                    data.Mode = (ForceMode)r.ReadInt32();
                    break;
                case PredictionRigidbody.ForceApplicationType.MovePosition:
                    data.Position = r.ReadVector3();
                    break;
                case PredictionRigidbody.ForceApplicationType.MoveRotation:
                    AutoPackType apt = (AutoPackType)r.ReadUInt8Unpacked();
                    data.Rotation = r.ReadQuaternion(apt);
                    break;
                default:
                    r.NetworkManager.LogError($"ForceApplicationType of {appType} is not supported.");
                    break;
            }

            fd.Data = data;
            return fd;
        }

        [DefaultWriter]
        public static void WritePredictionRigidbody(this Writer w, PredictionRigidbody pr)
        {
            w.Write(pr.Rigidbody.GetState(pr.RotationPacking));
            /* This used to write pr.GetPendingForces() but is no longer needed, assuming the user properly
             * reconciles everything that modifies the predictionRigidbody. */
            w.WriteList<PredictionRigidbody.EntryData>(null);
        }

        [DefaultReader]
        public static PredictionRigidbody ReadPredictionRigidbody(this Reader r)
        {
            List<PredictionRigidbody.EntryData> lst = CollectionCaches<PredictionRigidbody.EntryData>.RetrieveList();

            RigidbodyState rs = r.Read<RigidbodyState>();
            r.ReadList(ref lst);
            PredictionRigidbody pr = ResettableObjectCaches<PredictionRigidbody>.Retrieve();

            pr.SetReconcileData(rs, lst);
            return pr;
        }

        [DefaultDeltaWriter]
        public static bool WriteDeltaEntryData(this Writer w, PredictionRigidbody.EntryData value)
        {
            w.WriteEntryData(value);
            return true;
        }

        [DefaultDeltaReader]
        public static PredictionRigidbody.EntryData ReadDeltaEntryData(this Reader r) => r.ReadEntryData();

        [DefaultDeltaWriter]
        public static bool WriteDeltaPredictionRigidbody(this Writer w, PredictionRigidbody pr)
        {
            w.WritePredictionRigidbody(pr);
            return true;
        }

        [DefaultDeltaReader]
        public static PredictionRigidbody ReadDeltaPredictionRigidbody(this Reader r) => r.ReadPredictionRigidbody();
    }

    [UseGlobalCustomSerializer]
    [Preserve]
    public class PredictionRigidbody : IResettable
    {
        #region Types.
        public struct AllForceData
        {
            public ForceMode Mode;
            public Vector3 Vector3Force;
            public Vector3 Position;
            public Quaternion Rotation;
            [ExcludeSerialization]
            public readonly AutoPackType RotationPacking;
            public float FloatForce;
            public float Radius;
            public float UpwardsModifier;

            /// <summary>
            /// Used for MovePosition.
            /// </summary>
            public AllForceData(Vector3 position) : this()
            {
                Position = position;
            }

            /// <summary>
            /// Used for MoveRotation.
            /// </summary>
            public AllForceData(Quaternion rotation, AutoPackType rotationPacking) : this()
            {
                Rotation = rotation;
                RotationPacking = rotationPacking;
            }

            /// <summary>
            /// Used for Force and Torque.
            /// </summary>
            public AllForceData(Vector3 force, ForceMode mode) : this()
            {
                Vector3Force = force;
                Mode = mode;
            }

            /// <summary>
            /// Used for Position.
            /// </summary>
            public AllForceData(Vector3 force, Vector3 position, ForceMode mode) : this()
            {
                Vector3Force = force;
                Position = position;
                Mode = mode;
            }

            /// <summary>
            /// Used for Explosive.
            /// </summary>
            /// <param name = "force"></param>
            /// <param name = "position"></param>
            /// <param name = "radius"></param>
            /// <param name = "upwardsModifier"></param>
            /// <param name = "mode"></param>
            public AllForceData(float force, Vector3 position, float radius, float upwardsModifier, ForceMode mode) : this()
            {
                FloatForce = force;
                Position = position;
                Radius = radius;
                UpwardsModifier = upwardsModifier;
                Mode = mode;
            }
        }

        public interface IForceData { }

        // How the force was applied.
        [System.Flags]
        public enum ForceApplicationType : byte
        {
            AddForceAtPosition = 1 << 0,
            AddExplosiveForce = 1 << 1,
            AddForce = 1 << 2,
            AddRelativeForce = 1 << 3,
            AddTorque = 1 << 4,
            AddRelativeTorque = 1 << 5,
            MovePosition = 1 << 6,
            MoveRotation = 1 << 7,
        }

        [UseGlobalCustomSerializer]
        public struct EntryData
        {
            public ForceApplicationType Type;
            public AllForceData Data;

            public EntryData(ForceApplicationType type, AllForceData data)
            {
                Type = type;
                Data = data;
            }

            public EntryData(EntryData fd)
            {
                Type = fd.Type;
                Data = fd.Data;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Rigidbody which force is applied.
        /// </summary>
        public Rigidbody Rigidbody { get; private set; }
        /// <summary>
        /// Returns if there are any pending forces.
        /// </summary>
        public bool HasPendingForces => _pendingForces != null && _pendingForces.Count > 0;
        #endregion

        #region Internal.
        /// <summary>
        /// RigidbodyState set only as reconcile data.
        /// </summary>
        [System.NonSerialized]
        internal RigidbodyState RigidbodyState;
        /// <summary>
        /// How much to pack rotation.
        /// </summary>
        [ExcludeSerialization]
        internal AutoPackType RotationPacking = AutoPackType.Packed;
        #endregion

        #region Private
        /// <summary>
        /// Forces waiting to be applied.
        /// </summary>
        [ExcludeSerialization]
        private List<EntryData> _pendingForces;

        /// <summary>
        /// Returns current pending forces.
        /// Modifying this collection could cause undesirable results.
        /// </summary>
        public List<EntryData> GetPendingForces() => _pendingForces;
        #endregion

        ~PredictionRigidbody()
        {
            if (_pendingForces != null)
                CollectionCaches<EntryData>.StoreAndDefault(ref _pendingForces);

            Rigidbody = null;
        }

        /// <summary>
        /// Rigidbody which force is applied.
        /// </summary>
        /// <param name = "rb"></param>
        public void Initialize(Rigidbody rb, AutoPackType rotationPacking = AutoPackType.Packed)
        {
            Rigidbody = rb;
            RotationPacking = rotationPacking;

            if (_pendingForces == null)
                _pendingForces = CollectionCaches<EntryData>.RetrieveList();
            else
                _pendingForces.Clear();
        }

        /// <summary>
        /// Adds Velocity force to the Rigidbody.
        /// </summary>
        public void AddForce(Vector3 force, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddForce, new(force, mode));
            _pendingForces.Add(fd);
        }

        public void AddRelativeForce(Vector3 force, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddRelativeForce, new(force, mode));
            _pendingForces.Add(fd);
        }

        public void AddTorque(Vector3 force, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddTorque, new(force, mode));
            _pendingForces.Add(fd);
        }

        public void AddRelativeTorque(Vector3 force, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddRelativeTorque, new(force, mode));
            _pendingForces.Add(fd);
        }

        public void AddExplosiveForce(float force, Vector3 position, float radius, float upwardsModifier = 0f, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddExplosiveForce, new(force, position, radius, upwardsModifier, mode));
            _pendingForces.Add(fd);
        }

        public void AddForceAtPosition(Vector3 force, Vector3 position, ForceMode mode = ForceMode.Force)
        {
            EntryData fd = new(ForceApplicationType.AddForceAtPosition, new(force, position, mode));
            _pendingForces.Add(fd);
        }

        /// <summary>
        /// Sets velocity while clearing pending forces.
        /// Simulate should still be called normally.
        /// </summary>
        public void Velocity(Vector3 force)
        {
            #if UNITY_6000_1_OR_NEWER
            Rigidbody.linearVelocity = force;
            #else
            Rigidbody.velocity = force;
            #endif
            RemoveForces(nonAngular: true);
        }

        /// <summary>
        /// Sets angularVelocity while clearing pending forces.
        /// Simulate should still be called normally.
        /// </summary>
        public void AngularVelocity(Vector3 force)
        {
            Rigidbody.angularVelocity = force;
            RemoveForces(nonAngular: false);
        }

        /// <summary>
        /// Moves the kinematic Rigidbody towards position.
        /// </summary>
        /// <param name="position">Next position.</param>
        public void MovePosition(Vector3 position)
        {
            EntryData fd = new(ForceApplicationType.MovePosition, new(position));
            _pendingForces.Add(fd);
        }

        /// <summary>
        /// Moves the kinematic Rigidbody towards rotation.
        /// </summary>
        /// <param name="position">Next position.</param>
        public void MoveRotation(Quaternion rotation)
        {
            EntryData fd = new(ForceApplicationType.MoveRotation, new(rotation, RotationPacking));
            _pendingForces.Add(fd);
        }

        /// <summary>
        /// Applies pending forces to rigidbody in the order they were added.
        /// </summary>
        public void Simulate()
        {
            foreach (EntryData item in _pendingForces)
            {
                AllForceData data = item.Data;
                switch (item.Type)
                {
                    case ForceApplicationType.AddTorque:
                        Rigidbody.AddTorque(data.Vector3Force, data.Mode);
                        break;
                    case ForceApplicationType.AddForce:
                        Rigidbody.AddForce(data.Vector3Force, data.Mode);
                        break;
                    case ForceApplicationType.AddRelativeTorque:
                        Rigidbody.AddRelativeTorque(data.Vector3Force, data.Mode);
                        break;
                    case ForceApplicationType.AddRelativeForce:
                        Rigidbody.AddRelativeForce(data.Vector3Force, data.Mode);
                        break;
                    case ForceApplicationType.AddExplosiveForce:
                        Rigidbody.AddExplosionForce(data.FloatForce, data.Position, data.Radius, data.UpwardsModifier, data.Mode);
                        break;
                    case ForceApplicationType.AddForceAtPosition:
                        Rigidbody.AddForceAtPosition(data.Vector3Force, data.Position, data.Mode);
                        break;
                    case ForceApplicationType.MovePosition:
                        Rigidbody.MovePosition(data.Position);
                        break;
                    case ForceApplicationType.MoveRotation:
                        Rigidbody.MoveRotation(data.Rotation);
                        break;
                }
            }
            _pendingForces.Clear();
        }

        /// <summary>
        /// Clears current and pending forces for velocity and angularVelocity.
        /// </summary>
        public void ClearVelocities()
        {
            Velocity(Vector3.zero);
            AngularVelocity(Vector3.zero);
        }

        /// <summary>
        /// Clears pending forces for velocity, or angular velocity.
        /// </summary>
        /// <param name = "nonAngular">True to clear pending velocity forces, false to clear pending angularVelocity forces.</param>
        public void ClearPendingForces(bool nonAngular)
        {
            RemoveForces(nonAngular);
        }

        /// <summary>
        /// Clears pending forces for velocity and angularVelocity.
        /// </summary>
        public void ClearPendingForces()
        {
            _pendingForces.Clear();
        }

        /// <summary>
        /// Reconciles to a state.
        /// </summary>
        public void Reconcile(PredictionRigidbody pr)
        {
            _pendingForces.Clear();
            
            if (pr._pendingForces != null)
            {
                foreach (EntryData item in pr._pendingForces)
                    _pendingForces.Add(new(item));
            }
            
            // Set state.
            Rigidbody.SetState(pr.RigidbodyState);

            ResettableObjectCaches<PredictionRigidbody>.Store(pr);
        }

        /// <summary>
        /// Removes forces from pendingForces.
        /// </summary>
        /// <param name = "nonAngular">True to remove if velocity, false if to remove angular velocity.</param>
        private void RemoveForces(bool nonAngular)
        {
            if (_pendingForces.Count > 0)
            {
                ForceApplicationType velocityApplicationTypes = ForceApplicationType.AddRelativeForce | ForceApplicationType.AddForce | ForceApplicationType.AddExplosiveForce;

                List<EntryData> datasToKeep = CollectionCaches<EntryData>.RetrieveList();
                foreach (EntryData item in _pendingForces)
                {
                    if (VelocityApplicationTypesContains(item.Type) == !nonAngular || item.Type == ForceApplicationType.MovePosition || item.Type == ForceApplicationType.MoveRotation)
                        datasToKeep.Add(item);
                }
                // Add back to _pendingForces if changed.
                if (datasToKeep.Count != _pendingForces.Count)
                {
                    _pendingForces.Clear();
                    
                    foreach (EntryData item in datasToKeep)
                        _pendingForces.Add(item);
                }
                
                CollectionCaches<EntryData>.Store(datasToKeep);

                bool VelocityApplicationTypesContains(ForceApplicationType apt)
                {
                    return (velocityApplicationTypes & apt) == apt;
                }
            }
        }

        internal void SetReconcileData(RigidbodyState rs, List<EntryData> lst)
        {
            RigidbodyState = rs;
            _pendingForces = lst;
        }

        public void ResetState()
        {
            CollectionCaches<EntryData>.StoreAndDefault(ref _pendingForces);
            Rigidbody = null;
        }

        public void InitializeState() { }
    }
}