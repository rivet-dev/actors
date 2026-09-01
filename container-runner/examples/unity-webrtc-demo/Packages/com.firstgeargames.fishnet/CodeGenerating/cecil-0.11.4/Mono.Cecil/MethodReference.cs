//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Text;
using System.Threading;

namespace MonoFN.Cecil
{
    public class MethodReference : MemberReference, IMethodSignature, IGenericParameterProvider, IGenericContext
    {
        internal ParameterDefinitionCollection parameters;
        private MethodReturnType return_type;
        internal Collection<GenericParameter> generic_parameters;
        public virtual bool HasThis { get; set; }
        public virtual bool ExplicitThis { get; set; }
        public virtual MethodCallingConvention CallingConvention { get; set; }
        public virtual bool HasParameters
        {
            get { return !parameters.IsNullOrEmpty(); }
        }
        public virtual Collection<ParameterDefinition> Parameters
        {
            get
            {
                if (parameters == null)
                    Interlocked.CompareExchange(ref parameters, new(this), null);

                return parameters;
            }
        }
        IGenericParameterProvider IGenericContext.Type
        {
            get
            {
                TypeReference declaring_type = DeclaringType;
                GenericInstanceType instance = declaring_type as GenericInstanceType;
                if (instance != null)
                    return instance.ElementType;

                return declaring_type;
            }
        }
        IGenericParameterProvider IGenericContext.Method
        {
            get { return this; }
        }
        GenericParameterType IGenericParameterProvider.GenericParameterType
        {
            get { return GenericParameterType.Method; }
        }
        public virtual bool HasGenericParameters
        {
            get { return !generic_parameters.IsNullOrEmpty(); }
        }
        public virtual Collection<GenericParameter> GenericParameters
        {
            get
            {
                if (generic_parameters == null)
                    Interlocked.CompareExchange(ref generic_parameters, new GenericParameterCollection(this), null);

                return generic_parameters;
            }
        }
        public TypeReference ReturnType
        {
            get
            {
                MethodReturnType return_type = MethodReturnType;
                return return_type != null ? return_type.ReturnType : null;
            }
            set
            {
                MethodReturnType return_type = MethodReturnType;
                if (return_type != null)
                    return_type.ReturnType = value;
            }
        }
        public virtual MethodReturnType MethodReturnType
        {
            get { return return_type; }
            set { return_type = value; }
        }
        public override string FullName
        {
            get
            {
                StringBuilder builder = new();
                builder.Append(ReturnType.FullName).Append(" ").Append(MemberFullName());
                this.MethodSignatureFullName(builder);
                return builder.ToString();
            }
        }
        public virtual bool IsGenericInstance
        {
            get { return false; }
        }
        public override bool ContainsGenericParameter
        {
            get
            {
                if (ReturnType.ContainsGenericParameter || base.ContainsGenericParameter)
                    return true;

                if (!HasParameters)
                    return false;

                Collection<ParameterDefinition> parameters = Parameters;

                for (int i = 0; i < parameters.Count; i++)
                    if (parameters[i].ParameterType.ContainsGenericParameter)
                        return true;

                return false;
            }
        }

        internal MethodReference()
        {
            return_type = new(this);
            token = new(TokenType.MemberRef);
        }

        public MethodReference(string name, TypeReference returnType) : base(name)
        {
            Mixin.CheckType(returnType, Mixin.Argument.returnType);

            return_type = new(this);
            return_type.ReturnType = returnType;
            token = new(TokenType.MemberRef);
        }

        public MethodReference(string name, TypeReference returnType, TypeReference declaringType) : this(name, returnType)
        {
            Mixin.CheckType(declaringType, Mixin.Argument.declaringType);

            DeclaringType = declaringType;
        }

        public virtual MethodReference GetElementMethod()
        {
            return this;
        }

        protected override IMemberDefinition ResolveDefinition()
        {
            return Resolve();
        }

        public new virtual MethodDefinition Resolve()
        {
            ModuleDefinition module = Module;
            if (module == null)
                throw new NotSupportedException();

            return module.Resolve(this);
        }
    }

    internal static partial class Mixin
    {
        public static bool IsVarArg(this IMethodSignature self)
        {
            return self.CallingConvention == MethodCallingConvention.VarArg;
        }

        public static int GetSentinelPosition(this IMethodSignature self)
        {
            if (!self.HasParameters)
                return -1;

            Collection<ParameterDefinition> parameters = self.Parameters;
            for (int i = 0; i < parameters.Count; i++)
                if (parameters[i].ParameterType.IsSentinel)
                    return i;

            return -1;
        }
    }
}