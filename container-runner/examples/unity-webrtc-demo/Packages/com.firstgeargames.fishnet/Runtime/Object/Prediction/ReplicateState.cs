using System;
using FishNet.Utility;
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object.Prediction
{
    [Flags]
    public enum ReplicateState : byte
    {
        /// <summary>
        /// The default value of this state.
        /// This value should never occur when a replicate runs.
        /// </summary>
        Invalid = 0,
        /// <summary>
        /// Server and clients use this flag.
        /// Flag will be set if data tick has run outside a reconcile, such as from user code within OnTick.
        /// </summary>
        Ticked = 1 << 0, // 1
        /// <summary>
        /// Only client will use this flag.
        /// Flag is set if data is being run during a reconcile.
        /// </summary>
        Replayed = 1 << 1, // 2
        /// <summary>
        /// Server and client use this flag.
        /// Data has been created by the server or client.
        /// This indicates that data is known and was intentionally sent.
        /// </summary>
        Created = 1 << 2 // 4
    }

    public static class ReplicateStateExtensions
    {
        /// <summary>
        /// Returns if value is valid.
        /// This should never be false.
        /// </summary>
        public static bool IsValid(this ReplicateState value) => value != ReplicateState.Invalid;

        /// <summary>
        /// Returns if value contains ReplicateState.Ticked.
        /// </summary>
        public static bool ContainsTicked(this ReplicateState value) => value.FastContains(ReplicateState.Ticked);

        /// <summary>
        /// Returns if value contains ReplicateState.Created.
        /// </summary>
        public static bool ContainsCreated(this ReplicateState value) => value.FastContains(ReplicateState.Created);

        /// <summary>
        /// Returns if value contains ReplicateState.Replayed.
        /// </summary>
        public static bool ContainsReplayed(this ReplicateState value) => value.FastContains(ReplicateState.Replayed);

        [Obsolete("Use ContainsReplayed.")]
        public static bool IsReplayed(this ReplicateState value) => value.ContainsReplayed();

        /// <summary>
        /// Returns if value is (ReplicateState.Ticked | ReplicateState.Created).
        /// </summary>
        public static bool IsTickedCreated(this ReplicateState value) => value == (ReplicateState.Ticked | ReplicateState.Created);

        /// <summary>
        /// Returns if value equals ReplicateState.Ticked.
        /// </summary>
        public static bool IsTickedNonCreated(this ReplicateState value) => value == ReplicateState.Ticked;

        /// <summary>
        /// Returns if value is (ReplicateState.Replayed | ReplicateState.Ticked | ReplicateState.Created).
        /// </summary>
        public static bool IsReplayedCreated(this ReplicateState value) => value == (ReplicateState.Replayed | ReplicateState.Created);

        /// <summary>
        /// Returns if value is ReplicateState.Replayed without ReplicateState.Ticked nor ReplicateState.Created.
        /// </summary>
        public static bool IsFuture(this ReplicateState value) => value == ReplicateState.Replayed;

        [Obsolete("Use ContainsCreated.")]
        public static bool IsCreated(this ReplicateState value) => value.ContainsCreated();

        /// <summary>
        /// True if part is containined within whole.
        /// </summary>
        public static bool FastContains(this ReplicateState whole, ReplicateState part) => (whole & part) == part;
    }
}
