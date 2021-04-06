import axios from 'axios'
import consts from '@/consts.js'

class Resource {
    constructor({
        api,
        endpoint,
        supportedMethods,
        axiosParams,
        requiresAuth
    }) {
        this.api = api
        this.endpoint = endpoint
        this.supportedMethods = supportedMethods || null
        this.axiosParams = axiosParams || {}
        this.requiresAuth = true
        if (typeof requiresAuth !== 'undefined') {
            this.requiresAuth = requiresAuth
        }
    }

    checkSupportedMethods(method) {
        let supported = this.supportedMethods === null ||
            this.supportedMethods.includes(method)
        if (!supported) {
            throw `Endpoint "${this.endpoint}" does not support the HTTP ` +
                `method ${method}`
        }
    }

    async checkAuth() {
        if (this.requiresAuth && !this.api.isAuthenticated()) {
            if (!this.api.store) {
              throw new Error("API is not authenticated and has no store!")
            }
            await this.api.store.dispatch("fetchToken")
        }
    }

    async get(params={}) {
        await this.checkAuth()
        const method = 'GET'
        this.checkSupportedMethods(method)
        let cacheKey = `${method}:${this.endpoint}:${JSON.stringify(params)}`
        let request = this.api.promiseCache[cacheKey]
        if (!request) {
          let axiosParams = { url: this.endpoint, params, ...this.axiosParams}
          request = this.api.axios(axiosParams)
          this.api.promiseCache[cacheKey] = request
        }
        let result = await request
        delete this.api.promiseCache[cacheKey]
        return result
    }

    async post(data = undefined, params = {}) {
        await this.checkAuth()
        this.checkSupportedMethods('POST')
        let axiosParams = {
          method: 'POST',
          url: this.endpoint,
          data,
          params,
          ...this.axiosParams
        }
        return await this.api.axios(axiosParams)
    }

}

export default class Api {
    constructor(host, version) {
        this.store = null
        this.domain = `${consts.api.protocol}//${host}`
        this.promiseCache = {}
        this.axios = axios.create({
            baseURL: `${this.domain}/api/v${version}`
        })
    }

    init(store) {
        this.store = store
    }

    isAuthenticated() {
        return this.store !== null && this.store.getters.isLoggedIn
    }

    updateAuth(token) {
        if (token === null) {
            delete this.axios.defaults.headers.common['Authorization']
        } else {
            let header = "Bearer " + token
            this.axios.defaults.headers.common['Authorization'] = header
        }
    }

    createResource(endpoint, params = {}) {
        return new Resource({
            api: this,
            endpoint: endpoint,
            ...params
        })
    }

    bot_status() {
        return this.createResource('/bot/status', {
            requiresAuth: false,
            supportedMethods: ['GET']
        })
    }

    // OAuth API endpoints are not vesioned
    authToken() {
        return this.createResource('/oauth/refresh', {
            axiosParams: { baseURL: `${this.domain}/api` },
            requiresAuth: false,
            supportedMethods: ['GET']
        })
    }

    oauthLogin() {
        return this.createResource('/oauth/token', {
            axiosParams: { baseURL: `${this.domain}/api` },
            requiresAuth: false,
            supportedMethods: ['POST']
        })
    }

    oauthLogout() {
        return this.createResource('/oauth/logout', {
            axiosParams: { baseURL: `${this.domain}/api` },
            requiresAuth: false,
            supportedMethods: ['POST']
        })
    }

    me() {
        return this.createResource('/users/@me', {
            supportedMethods: ['GET']
        })
    }

    guilds() {
        return this.createResource('/users/@me/guilds', {
            supportedMethods: ['GET']
        })
    }

    guildConfig(guildId) {
        return this.createResource(`/guild/${guildId}`)
    }

    loggingConfig(guildId) {
        return this.createResource(`/guild/${guildId}/logging.json`)
    }

    autoConfig(guildId) {
        return this.createResource(`/guild/${guildId}/auto.json`)
    }

    moderationConfig(guildId) {
        return this.createResource(`/guild/${guildId}/moderation.json`)
    }

    musicConfig(guildId) {
        return this.createResource(`/guild/${guildId}/music.json`)
    }

    announceConfig(guildId) {
        return this.createResource(`/guild/${guildId}/announce.json`)
    }

    roleConfig(guildId) {
        return this.createResource(`/guild/${guildId}/role.json`)
    }
}
