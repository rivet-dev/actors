using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.CodeGenerating.Processing.Rpc;
using FishNet.Configuring;
using FishNet.Managing.Logging;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.Collections.Generic;
using System.Linq;

namespace FishNet.CodeGenerating.Processing
{
    internal class QolAttributeProcessor : CodegenBase
    {
        internal bool Process(TypeDefinition typeDef, bool moveStrippedCalls)
        {
            bool modified = false;
            List<MethodDefinition> methods = typeDef.Methods.ToList();


            foreach (MethodDefinition md in methods)
            {
                // Has RPC attribute, doesn't quality for a quality of life attribute.
                if (GetClass<RpcProcessor>().Attributes.HasRpcAttributes(md))
                    continue;

                QolAttributeType qolType;
                CustomAttribute qolAttribute = GetQOLAttribute(md, out qolType);
                if (qolAttribute == null)
                    continue;

                /* This is a one time check to make sure the qolType is
                 * a supported value. Multiple methods beyond this rely on the
                 * value being supported. Rather than check in each method a
                 * single check is performed here. */
                if (qolType != QolAttributeType.Server && qolType != QolAttributeType.Client)
                {
                    LogError($"QolAttributeType of {qolType.ToString()} is unhandled.");
                    continue;
                }

                CreateAttributeMethod(md, qolAttribute, qolType);
                modified = true;
            }

            return modified;
        }

        /// <summary>
        /// Returns the RPC attribute on a method, if one exist. Otherwise returns null.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <param name = "rpcType"></param>
        /// <returns></returns>
        private CustomAttribute GetQOLAttribute(MethodDefinition methodDef, out QolAttributeType qolType)
        {
            CustomAttribute foundAttribute = null;
            qolType = QolAttributeType.None;
            // Becomes true if an error occurred during this process.
            bool error = false;
            // Nothing to check.
            if (methodDef == null || methodDef.CustomAttributes == null)
                return null;

            foreach (CustomAttribute customAttribute in methodDef.CustomAttributes)
            {
                QolAttributeType thisQolType = GetClass<AttributeHelper>().GetQolAttributeType(customAttribute.AttributeType.FullName);
                if (thisQolType != QolAttributeType.None)
                {
                    // A qol attribute already exist.
                    if (foundAttribute != null)
                    {
                        LogError($"{methodDef.Name} {thisQolType.ToString()} method cannot have multiple quality of life attributes.");
                        error = true;
                    }
                    ////Static method.
                    // if (methodDef.IsStatic)
                    // {
                    //    CodegenSession.AddError($"{methodDef.Name} {thisQolType.ToString()} method cannot be static.");
                    //    error = true;
                    // }
                    // Abstract method.
                    if (methodDef.IsAbstract)
                    {
                        LogError($"{methodDef.Name} {thisQolType.ToString()} method cannot be abstract.");
                        error = true;
                    }

                    // If all checks passed.
                    if (!error)
                    {
                        foundAttribute = customAttribute;
                        qolType = thisQolType;
                    }
                }
            }

            // If an error occurred then reset results.
            if (error)
            {
                foundAttribute = null;
                qolType = QolAttributeType.None;
            }

            return foundAttribute;
        }

        /// <summary>
        /// Modifies the specified method to use QolType.
        /// </summary>
        private void CreateAttributeMethod(MethodDefinition methodDef, CustomAttribute qolAttribute, QolAttributeType qolType)
        {
            bool inheritsNetworkBehaviour = methodDef.DeclaringType.InheritsNetworkBehaviour(Session);

            // True to use InstanceFInder.
            bool useStatic = methodDef.IsStatic || !inheritsNetworkBehaviour;
            // Check to force using static.
            if (!useStatic && qolAttribute.GetField("UseIsStarted", false))
                useStatic = true;

            if (qolType == QolAttributeType.Client)
            {
                if (!StripMethod(methodDef))
                {
                    LoggingType logging = qolAttribute.GetField("Logging", LoggingType.Warning);
                    /* Since isClient also uses insert first
                     * it will be put ahead of the IsOwner check, since the
                     * codegen processes it after IsOwner. EG...
                     * IsOwner will be added first, then IsClient will be added first over IsOwner. */
                    bool requireOwnership = qolAttribute.GetField("RequireOwnership", false);
                    if (requireOwnership && useStatic)
                    {
                        LogError($"Method {methodDef.Name} has a [Client] attribute which requires ownership but the method may not use this attribute. Either the method is static, or the script does not inherit from NetworkBehaviour.");
                        return;
                    }
                    // If (!base.IsOwner);
                    if (requireOwnership)
                        GetClass<NetworkBehaviourHelper>().CreateLocalClientIsOwnerCheck(methodDef, logging, true, false, true);
                    // Otherwise normal IsClient check.
                    else
                        GetClass<NetworkBehaviourHelper>().CreateIsClientCheck(methodDef, logging, useStatic, true, !useStatic);
                }
            }
            else if (qolType == QolAttributeType.Server)
            {
                if (!StripMethod(methodDef))
                {
                    LoggingType logging = qolAttribute.GetField("Logging", LoggingType.Warning);
                    GetClass<NetworkBehaviourHelper>().CreateIsServerCheck(methodDef, logging, useStatic, true, !useStatic);
                }
            }

            bool StripMethod(MethodDefinition md)
            {

                // Fall through.
                return false;
            }
        }

    }
}